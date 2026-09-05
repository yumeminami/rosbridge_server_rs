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
use std::time::Duration;

#[derive(Clone)]
struct Sender {
    channel: std::sync::mpsc::SyncSender<Command>,
    wake: rosbridge_server_rs::ros::Wake,
}
impl Sender {
    fn try_send(&self, command: Command) -> Result<()> {
        self.channel.try_send(command)?;
        self.wake.trigger();
        Ok(())
    }
    fn send(&self, command: Command) -> Result<()> {
        self.channel.send(command)?;
        self.wake.trigger();
        Ok(())
    }
}

enum Command {
    Connect(u64, rosbridge_server_rs::bridge::Output),
    Message(u64, serde_json::Value),
    Disconnect(u64),
    Shutdown,
}
pub(super) async fn run(args: Args) -> Result<()> {
    use rosbridge_server_rs::{bridge::Bridge, ros::Ros};
    use std::sync::mpsc;
    use tokio::{net::TcpListener, task::JoinSet};
    let timeout =
        Duration::try_from_secs_f64(args.service_timeout).context("invalid service timeout")?;
    anyhow::ensure!(
        args.max_message_size > 0,
        "max-message-size must be positive"
    );
    let access = rosbridge_server_rs::access::Access::from_ros_args(&args.ros_args)?;
    let (sender, receiver) = mpsc::sync_channel(args.incoming_queue_size);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let max = args.max_message_size;
    let write_queue = args.write_queue_size;
    let fragment_timeout = Duration::from_secs_f64(args.fragment_timeout);
    let url_path = args.url_path.clone();
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
                let wait = bridge.next_wakeup();
                bridge.backend.wait(wait)?;
                for index in 0..64 {
                    if index == 63 {
                        bridge.backend.wake_handle().trigger();
                    }
                    let command = match receiver.try_recv() {
                        Ok(command) => Some(command),
                        Err(mpsc::TryRecvError::Empty) => None,
                        Err(mpsc::TryRecvError::Disconnected) => Some(Command::Shutdown),
                    };
                    let Some(command) = command else {
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
            let _ = sender.send(Command::Shutdown);
            let _ = worker.join();
            return Err(e.into());
        }
    };
    tracing::info!(address=%listener.local_addr()?,"rosbridge WebSocket server listening");
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    let mut connections = JoinSet::new();
    let mut next = 0;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                next += 1;
                let tx = sender.clone();
                let url_path = url_path.clone();
                connections.spawn(async move {
                    if let Err(e) = connection(stream, next, max, write_queue, fragment_timeout, url_path, tx).await {
                        tracing::warn!(connection = next, %peer, "connection ended: {e:#}");
                    }
                });
            }
            _ = &mut shutdown => break,
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
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    let _ = sender.send(Command::Shutdown);
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("ROS worker panicked"))??;
    Ok(())
}
async fn connection(
    stream: tokio::net::TcpStream,
    id: u64,
    max: usize,
    write_queue: usize,
    fragment_timeout: Duration,
    url_path: String,
    sender: Sender,
) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use rosbridge_server_rs::wire::Decoder;
    use tokio_tungstenite::{
        accept_hdr_async_with_config,
        tungstenite::{
            Message,
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
    let connected_at = std::time::Instant::now();
    let (mut sink, mut source) = websocket.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(write_queue);
    sender.try_send(Command::Connect(id, out_tx))?;
    let mut decoder = Decoder::with_timeout(fragment_timeout);
    let mut close_code = None;
    let mut close_reason = String::new();
    let result = async {
        loop {
            tokio::select! {
                frame = source.next() => match frame {
                    Some(Ok(Message::Close(frame))) => {
                        if let Some(frame) = frame {
                            close_code = Some(u16::from(frame.code));
                            close_reason = frame.reason.into_owned();
                        }
                        break;
                    }
                    None => break,
                    Some(Ok(Message::Ping(value))) => {
                        tokio::time::timeout(Duration::from_secs(10), sink.send(Message::Pong(value))).await??;
                    }
                    Some(Ok(frame)) => match decoder.decode(frame, max) {
                        Ok(Some(value)) => sender.try_send(Command::Message(id, value))?,
                        Ok(None) => {},
                        Err(e) => {
                            let error = serde_json::json!({"op":"status", "level":"error", "msg":e.to_string()});
                            tokio::time::timeout(Duration::from_secs(10), sink.send(Message::Text(error.to_string()))).await??;
                        }
                    },
                    Some(Err(e)) => return Err(anyhow::Error::from(e)),
                },
                frames = out_rx.recv() => match frames {
                    Some(frames) => {
                        for frame in frames {
                            tokio::time::timeout(Duration::from_secs(10), sink.send(frame)).await??;
                        }
                    }
                    None => break,
                },
            }
        }
        Ok(())
    }.await;
    tracing::info!(
        connection = id,
        %peer,
        duration_seconds = connected_at.elapsed().as_secs_f64(),
        ?close_code,
        %close_reason,
        "WebSocket session ended"
    );
    // Closing the receiver also lets the ROS worker detect disconnect if its queue is full.
    drop(out_rx);
    let _ = sender.try_send(Command::Disconnect(id));
    result
}
