//
// Copyright (c) 2026 Wing Mun Fung
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0, available at
// https://www.eclipse.org/legal/epl-2.0/, or the Apache License, Version 2.0,
// available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//

//! WebSocket tasks and the single ROS worker thread.

use crate::Args;
use anyhow::{Context, Result};
use rosbridge_server_rs::incoming::{self, Command};
use std::time::Duration;

#[derive(Clone)]
struct Sender {
    channel: incoming::Sender,
    wake: rosbridge_server_rs::ros::Wake,
}
impl Sender {
    fn try_send(&self, command: Command) -> Result<()> {
        match command {
            Command::Connect(id, output) => self.channel.connect(id, output)?,
            Command::Message(id, value) => self.channel.message(id, value)?,
            Command::Disconnect(id) => self.channel.disconnect(id),
            Command::Shutdown => self.channel.shutdown(),
        }
        self.wake.trigger();
        Ok(())
    }
}
// Cancellation and task aborts must also remove their scheduler entry.
struct Registration {
    sender: Sender,
    id: u64,
}
impl Drop for Registration {
    fn drop(&mut self) {
        let _ = self.sender.try_send(Command::Disconnect(self.id));
    }
}

struct ConnectionOptions {
    max: usize,
    write_queue: usize,
    write_queue_bytes: usize,
    fragment_timeout: Duration,
    url_path: String,
    timing: rosbridge_server_rs::websocket::Timing,
    encoding_pool: rosbridge_server_rs::encoding::Pool,
}

pub(super) async fn run(args: Args) -> Result<()> {
    use rosbridge_server_rs::{bridge::Bridge, ros::Ros};
    use tokio::{net::TcpListener, task::JoinSet};
    let mut shutdown = crate::shutdown::Signals::new()?;
    let timeout =
        Duration::try_from_secs_f64(args.service_timeout).context("invalid service timeout")?;
    anyhow::ensure!(
        args.max_message_size > 0,
        "max-message-size must be positive"
    );
    let access = rosbridge_server_rs::access::Access::from_ros_args(&args.ros_args)?;
    let (sender, receiver) = incoming::channel(args.incoming_queue_size);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let max = args.max_message_size;
    let write_queue = args.write_queue_size;
    let write_queue_bytes = args.write_queue_bytes;
    let fragment_timeout = Duration::from_secs_f64(args.fragment_timeout);
    let url_path = args.url_path.clone();
    let encoding_pool = rosbridge_server_rs::encoding::Pool::new(args.encoding_workers);
    let timing = rosbridge_server_rs::websocket::Timing {
        ping_interval: Duration::try_from_secs_f64(args.websocket_ping_interval)?,
        ping_timeout: Duration::try_from_secs_f64(args.websocket_ping_timeout)?,
        ..Default::default()
    };
    let worker = std::thread::Builder::new()
        .name("rosbridge-rcl".into())
        .spawn(move || -> Result<()> {
            let mut backend = match Ros::new(
                &args.node_name,
                &args.namespace,
                args.use_sim_time,
                &args.ros_args,
            ) {
                Ok(ros) => ros,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("{e:#}")));
                    return Err(e);
                }
            };
            if !args.no_rosapi
                && let Err(error) = backend.enable_rosapi(&args.ros_args)
            {
                let _ = ready_tx.send(Err(format!("{error:#}")));
                return Err(error);
            }
            let mut bridge = Bridge::new(backend, timeout);
            bridge.access = access;
            let _ = ready_tx.send(Ok(bridge.backend.wake_handle()));
            loop {
                let wait = if receiver.has_pending() {
                    Duration::ZERO
                } else {
                    bridge.next_wakeup()
                };
                bridge.backend.wait(wait)?;
                for index in 0..64 {
                    if index == 63 {
                        bridge.backend.wake_handle().trigger();
                    }
                    let Some(command) = receiver.try_recv() else {
                        break;
                    };
                    match command {
                        Command::Connect(id, out) => bridge.connect(id, out),
                        Command::Message(id, v) => bridge.command(id, v),
                        Command::Disconnect(id) => bridge.disconnect(id),
                        Command::Shutdown => {
                            bridge.shutdown();
                            return Ok(());
                        }
                    }
                }
                bridge.tick()?;
            }
        })?;
    let wake = ready_rx.await?.map_err(anyhow::Error::msg)?;
    let sender = Sender {
        channel: sender,
        wake,
    };
    let listener = match TcpListener::bind(args.bind).await {
        Ok(l) => l,
        Err(e) => {
            let _ = sender.try_send(Command::Shutdown);
            let _ = worker.join();
            return Err(e.into());
        }
    };
    tracing::info!(address=%listener.local_addr()?,"rosbridge WebSocket server listening");
    let mut connections = JoinSet::new();
    let mut next = 0;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                next += 1;
                let tx = sender.clone();
                let url_path = url_path.clone();
                let encoding_pool = encoding_pool.clone();
                connections.spawn(async move {
                    let options = ConnectionOptions { max, write_queue, write_queue_bytes, fragment_timeout, url_path, timing, encoding_pool };
                    if let Err(e) = connection(stream, next, options, tx).await {
                        tracing::warn!(connection = next, %peer, "connection ended: {e:#}");
                    }
                });
            }
            _ = shutdown.recv() => {
                tracing::info!("Shutdown signal received");
                break;
            },
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(e) = result {
                    tracing::warn!("connection task failed: {e}");
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if worker.is_finished() {
                    break;
                }
            }
        }
    }
    let _ = sender.try_send(Command::Shutdown);
    if tokio::time::timeout(Duration::from_secs(12), async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("ROS worker panicked"))??;
    Ok(())
}
async fn connection(
    stream: tokio::net::TcpStream,
    id: u64,
    options: ConnectionOptions,
    sender: Sender,
) -> Result<()> {
    let ConnectionOptions {
        max,
        write_queue,
        write_queue_bytes,
        fragment_timeout,
        url_path,
        timing,
        encoding_pool,
    } = options;
    use tokio_tungstenite::{
        accept_hdr_async_with_config,
        tungstenite::{
            handshake::server::{Request, Response},
            protocol::WebSocketConfig,
        },
    };
    let peer = stream.peer_addr()?;
    stream.set_nodelay(true)?;
    let config = WebSocketConfig {
        max_message_size: Some(max),
        max_frame_size: Some(max),
        ..Default::default()
    };
    let websocket = tokio::time::timeout(
        Duration::from_secs(10),
        accept_hdr_async_with_config(
            stream,
            |request: &Request, response: Response| {
                if request.uri().path() != url_path {
                    let mut error = tokio_tungstenite::tungstenite::http::Response::new(Some(
                        "WebSocket path not found".into(),
                    ));
                    *error.status_mut() =
                        tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
                    return Err(error);
                }
                let header = |name| {
                    request
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                };
                tracing::info!(
                    connection = id,
                    %peer,
                    path = request.uri().path(),
                    origin = header("origin"),
                    user_agent = header("user-agent"),
                    forwarded_for = header("x-forwarded-for"),
                    "WebSocket client handshake"
                );
                Ok(response)
            },
            Some(config),
        ),
    )
    .await??;
    let (out_tx, out_rx) = rosbridge_server_rs::outgoing::channel_with_pool(
        write_queue,
        write_queue_bytes,
        encoding_pool,
    );
    let registration = sender.try_send(Command::Connect(id, out_tx));
    if let Err(error) = &registration {
        tracing::warn!(connection = id, %error, "ROS connection command queue unavailable");
    }
    let _registration_guard = Registration {
        sender: sender.clone(),
        id,
    };
    // A rejected registration drops Output; run still performs a closing handshake.
    let result = rosbridge_server_rs::websocket::run_with_timing(
        websocket,
        id,
        max,
        fragment_timeout,
        out_rx,
        |value| sender.try_send(Command::Message(id, value)),
        timing,
    )
    .await;
    // The receiver is closed even if the command queue is full; tick also reaps it.
    let _ = sender.try_send(Command::Disconnect(id));
    result.and(registration)
}
