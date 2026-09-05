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
    let (sender, receiver) = mpsc::sync_channel(256);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let max = args.max_message_size;
    let worker = std::thread::Builder::new()
        .name("rosbridge-rcl".into())
        .spawn(move || -> Result<()> {
            let backend = match Ros::new(
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
            let mut bridge = Bridge::new(backend, timeout);
            let _ = ready_tx.send(Ok(()));
            loop {
                for i in 0..64 {
                    let command = if i == 0 {
                        match receiver.recv_timeout(Duration::from_millis(2)) {
                            Ok(c) => Some(c),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => Some(Command::Shutdown),
                        }
                    } else {
                        receiver.try_recv().ok()
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
    ready_rx.await?.map_err(anyhow::Error::msg)?;
    let listener = match TcpListener::bind(args.bind).await {
        Ok(l) => l,
        Err(e) => {
            let _ = sender.send(Command::Shutdown);
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
                connections.spawn(async move {
                    if let Err(e) = connection(stream, next, max, tx).await {
                        tracing::debug!(%peer, "connection ended: {e:#}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => break,
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
    sender: std::sync::mpsc::SyncSender<Command>,
) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use rosbridge_server_rs::wire::Decoder;
    use tokio_tungstenite::{
        accept_async_with_config,
        tungstenite::{Message, protocol::WebSocketConfig},
    };
    stream.set_nodelay(true)?;
    let config = WebSocketConfig {
        max_message_size: Some(max),
        max_frame_size: Some(max),
        ..Default::default()
    };
    let websocket = tokio::time::timeout(
        Duration::from_secs(10),
        accept_async_with_config(stream, Some(config)),
    )
    .await??;
    let (mut sink, mut source) = websocket.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(64);
    sender.try_send(Command::Connect(id, out_tx))?;
    let mut decoder = Decoder::default();
    let result = async {
        loop {
            tokio::select! {
                frame = source.next() => match frame {
                    Some(Ok(Message::Close(_))) | None => break,
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
    // Closing the receiver also lets the ROS worker detect disconnect if its queue is full.
    drop(out_rx);
    let _ = sender.try_send(Command::Disconnect(id));
    result
}
