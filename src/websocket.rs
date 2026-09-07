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

//! WebSocket I/O independent of ROS, including bounded closing handshakes.
use crate::{outgoing::Receiver, wire::Decoder};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio::{net::TcpStream, sync::Mutex};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

enum End {
    PeerClose(Option<CloseFrame<'static>>),
    Eof,
    OutputClosed,
    HeartbeatTimeout,
}

/// Heartbeat interval zero disables active probes. Write deadlines remain enabled.
#[derive(Clone, Copy)]
pub struct Timing {
    pub write_timeout: Duration,
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            write_timeout: WRITE_TIMEOUT,
            ping_interval: Duration::from_secs(30),
            ping_timeout: Duration::from_secs(30),
        }
    }
}

pub async fn run(
    websocket: WebSocketStream<TcpStream>,
    connection: u64,
    max: usize,
    fragment_timeout: Duration,
    output: Receiver,
    on_message: impl FnMut(serde_json::Value) -> Result<()>,
) -> Result<()> {
    run_with_timing(
        websocket,
        connection,
        max,
        fragment_timeout,
        output,
        on_message,
        Timing::default(),
    )
    .await
}

pub async fn run_with_timing(
    websocket: WebSocketStream<TcpStream>,
    connection: u64,
    max: usize,
    fragment_timeout: Duration,
    mut output: Receiver,
    mut on_message: impl FnMut(serde_json::Value) -> Result<()>,
    timing: Timing,
) -> Result<()> {
    let write_timeout = timing.write_timeout;
    let pending_ping = std::sync::Mutex::new(None::<Vec<u8>>);
    let pong_received = tokio::sync::Notify::new();
    let started = Instant::now();
    let (sink, mut source) = websocket.split();
    let sink = Mutex::new(sink);
    let mut decoder = Decoder::with_timeout(fragment_timeout);
    let result: Result<End> = {
        let reader = async {
            while let Some(frame) = source.next().await {
                match frame.context("WebSocket read failed")? {
                    Message::Close(frame) => return Ok(End::PeerClose(frame)),
                    Message::Ping(_) => {
                        // Tungstenite queued the automatic pong when reading the ping.
                        tokio::time::timeout(write_timeout, async {
                            sink.lock().await.flush().await
                        })
                        .await
                        .context("WebSocket pong write timed out")?
                        .context("WebSocket pong write failed")?;
                    }
                    Message::Pong(payload) => {
                        let mut pending = pending_ping.lock().unwrap();
                        if pending.as_ref() == Some(&payload) {
                            *pending = None;
                            pong_received.notify_one();
                        }
                    }
                    frame => match decoder.decode(frame, max) {
                        Ok(Some(value)) => {
                            on_message(value).context("ROS command queue unavailable")?
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let status = serde_json::json!({"op":"status", "level":"error", "msg":error.to_string()});
                            tokio::time::timeout(write_timeout, async {
                                sink.lock()
                                    .await
                                    .send(Message::Text(status.to_string()))
                                    .await
                            })
                            .await
                            .context("WebSocket status write timed out")?
                            .context("WebSocket status write failed")?;
                        }
                    },
                }
            }
            Ok(End::Eof)
        };
        let writer = async {
            while let Some(frames) = output.recv().await {
                // Keep a protocol batch intact, including all rosbridge fragments.
                tokio::time::timeout(write_timeout, async {
                    let mut sink = sink.lock().await;
                    for frame in frames {
                        sink.send(frame).await?;
                    }
                    Ok::<_, tokio_tungstenite::tungstenite::Error>(())
                })
                .await
                .context("WebSocket batch write timed out")?
                .context("WebSocket batch write failed")?;
            }
            Ok(End::OutputClosed)
        };
        let heartbeat = async {
            if timing.ping_interval.is_zero() {
                return std::future::pending::<Result<End>>().await;
            }
            let mut sequence = 0u64;
            loop {
                tokio::time::sleep(timing.ping_interval).await;
                sequence = sequence.wrapping_add(1);
                let payload = sequence.to_be_bytes().to_vec();
                *pending_ping.lock().unwrap() = Some(payload.clone());
                tokio::time::timeout(write_timeout, async {
                    sink.lock().await.send(Message::Ping(payload)).await
                })
                .await
                .context("WebSocket ping write timed out")?
                .context("WebSocket ping write failed")?;
                // Start the pong deadline after the ping has been flushed. Register
                // before sending so a quick pong cannot race the pending state.
                let acknowledged = async {
                    loop {
                        if pending_ping.lock().unwrap().is_none() {
                            return;
                        }
                        pong_received.notified().await;
                    }
                };
                if tokio::time::timeout(timing.ping_timeout, acknowledged)
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        connection,
                        timeout_seconds = timing.ping_timeout.as_secs_f64(),
                        "WebSocket pong timed out"
                    );
                    return Ok(End::HeartbeatTimeout);
                }
            }
        };
        tokio::select! { result = reader => result, result = writer => result, result = heartbeat => result }
    };
    let (local_code, reason) = match &result {
        Ok(End::PeerClose(_)) => (None, "peer close"),
        Ok(End::Eof) => (None, "peer stream ended"),
        Ok(End::HeartbeatTimeout) => (Some(CloseCode::Away), "WebSocket pong timed out"),
        Ok(End::OutputClosed) => match output.close_reason() {
            Some("outbound encoding failed") => {
                (Some(CloseCode::Error), "outbound encoding failed")
            }
            Some(reason) => (Some(CloseCode::Again), reason),
            None => (Some(CloseCode::Away), "bridge disconnected"),
        },
        Err(error) if error.downcast_ref::<crate::incoming::QueueFull>().is_some() => {
            (Some(CloseCode::Again), "client incoming queue full")
        }
        Err(error)
            if error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some() =>
        {
            (Some(CloseCode::Error), "WebSocket write timed out")
        }
        Err(_) => (Some(CloseCode::Error), "WebSocket session failed"),
    };
    // Release queued data immediately so the ROS worker can detect any failure.
    drop(output);
    let peer_frame = match &result {
        Ok(End::PeerClose(frame)) => frame.as_ref(),
        _ => None,
    };
    tracing::info!(connection, duration_seconds = started.elapsed().as_secs_f64(),
        peer_close_code = ?peer_frame.map(|f| u16::from(f.code)),
        peer_close_reason = peer_frame.map(|f| f.reason.as_ref()).unwrap_or(""),
        local_close_code = ?local_code.map(u16::from), reason, "WebSocket session ended");
    if let Err(error) = &result {
        tracing::warn!(connection, "WebSocket failure: {error:#}");
    }
    if !matches!(result, Ok(End::Eof)) {
        let closing = tokio::time::timeout(CLOSE_TIMEOUT, async {
            let mut sink = sink.lock().await;
            if let Some(code) = local_code {
                sink.send(Message::Close(Some(CloseFrame {
                    code,
                    reason: reason.into(),
                })))
                .await?;
                while let Some(frame) = source.next().await {
                    if matches!(frame?, Message::Close(_)) {
                        break;
                    }
                }
            }
            // Also flush the automatic Close response to a peer-initiated close.
            sink.flush().await
        })
        .await;
        match closing {
            Err(_) => tracing::warn!(connection, "WebSocket close handshake timed out"),
            Ok(Err(error)) => {
                tracing::debug!(connection, %error, "WebSocket close handshake failed")
            }
            Ok(Ok(())) => {}
        }
    }
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outgoing::{self, CONTROL_CAPACITY};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, connect_async};

    #[tokio::test]
    async fn overload_sends_close_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (tx, rx) = outgoing::channel(1);
            for _ in 0..CONTROL_CAPACITY {
                tx.send(1, "status", None, 0, vec![Message::Text("x".into())])
                    .unwrap();
            }
            assert!(
                tx.send(1, "status", None, 0, vec![Message::Text("x".into())])
                    .is_err()
            );
            run(ws, 1, 1024, Duration::from_secs(1), rx, |_| Ok(()))
                .await
                .unwrap();
        });
        let (mut client, _) = connect_async(format!("ws://{address}")).await.unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(2), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Close(Some(frame)) = frame else {
            panic!("expected Close frame")
        };
        assert_eq!(frame.code, CloseCode::Again);
        assert_eq!(frame.reason, "control send queue full");
        client.flush().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn peer_close_is_acknowledged() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (_tx, rx) = outgoing::channel(1);
            run(ws, 1, 1024, Duration::from_secs(1), rx, |_| Ok(()))
                .await
                .unwrap();
        });
        let (mut client, _) = connect_async(format!("ws://{address}")).await.unwrap();
        client
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "done".into(),
            })))
            .await
            .unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(2), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(frame, Message::Close(Some(frame)) if frame.code == CloseCode::Normal));
        server.await.unwrap();
    }
    #[tokio::test]
    async fn stalled_socket_write_times_out_and_closes_receiver() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (tx, rx) = outgoing::channel(1);
            tx.send(
                1,
                "publish",
                Some("/image"),
                0,
                vec![Message::Binary(vec![0; 32 * 1024 * 1024])],
            )
            .unwrap();
            let error = run_with_timing(
                ws,
                1,
                1024,
                Duration::from_secs(1),
                rx,
                |_| Ok(()),
                Timing {
                    write_timeout: Duration::from_millis(50),
                    ..Timing::default()
                },
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("batch write timed out"));
            assert!(tx.is_closed());
        });
        // Hold the TCP connection open without consuming its WebSocket data.
        let (_client, _) = connect_async(format!("ws://{address}")).await.unwrap();
        tokio::time::timeout(Duration::from_secs(4), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn stalled_writer_does_not_block_incoming_commands() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (tx, rx) = outgoing::channel(1);
            tx.send(
                1,
                "publish",
                Some("/image"),
                0,
                vec![Message::Binary(vec![0; 32 * 1024 * 1024])],
            )
            .unwrap();
            let mut observed_tx = Some(observed_tx);
            let _ = run(ws, 1, 1024, Duration::from_secs(1), rx, move |value| {
                observed_tx.take().unwrap().send(value).unwrap();
                anyhow::bail!("end test session")
            })
            .await;
        });
        let (mut client, _) = connect_async(format!("ws://{address}")).await.unwrap();
        client
            .send(Message::Text(
                r#"{"op":"unsubscribe","topic":"/image"}"#.into(),
            ))
            .await
            .unwrap();
        let value = tokio::time::timeout(Duration::from_secs(2), observed_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(value["op"], "unsubscribe");
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
    async fn heartbeat_server(timing: Timing) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (tx, rx) = outgoing::channel(1);
            run_with_timing(ws, 1, 1024, Duration::from_secs(1), rx, |_| Ok(()), timing)
                .await
                .unwrap();
            assert!(tx.is_closed());
        });
        (format!("ws://{address}"), server)
    }

    #[tokio::test]
    async fn matching_pongs_keep_connection_alive_across_multiple_probes() {
        let (url, server) = heartbeat_server(Timing {
            ping_interval: Duration::from_millis(20),
            ping_timeout: Duration::from_secs(1),
            ..Timing::default()
        })
        .await;
        let (mut client, _) = connect_async(url).await.unwrap();
        let mut previous = None;
        for _ in 0..3 {
            let frame = tokio::time::timeout(Duration::from_secs(2), client.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let Message::Ping(payload) = frame else {
                panic!("expected ping")
            };
            assert_ne!(previous.as_ref(), Some(&payload));
            previous = Some(payload);
            client.flush().await.unwrap(); // Flush tungstenite's matching automatic pong.
        }
        client.send(Message::Close(None)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match client.next().await.unwrap().unwrap() {
                    Message::Close(_) => break,
                    Message::Ping(_) => {} // A probe may already be in flight.
                    other => panic!("unexpected frame while closing: {other:?}"),
                }
            }
        })
        .await
        .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn wrong_pong_and_regular_traffic_do_not_extend_heartbeat_deadline() {
        let (url, server) = heartbeat_server(Timing {
            ping_interval: Duration::from_millis(20),
            ping_timeout: Duration::from_millis(100),
            ..Timing::default()
        })
        .await;
        let (mut client, _) = connect_async(url).await.unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(2), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(frame, Message::Ping(_)));
        // Explicit pong replaces the queued automatic reply, but doesn't match the probe.
        client.send(Message::Pong(vec![99])).await.unwrap();
        client
            .send(Message::Text(r#"{"op":"test"}"#.into()))
            .await
            .unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(2), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Close(Some(frame)) = frame else {
            panic!("expected timeout Close")
        };
        assert_eq!(frame.reason, "WebSocket pong timed out");
        client.flush().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn missing_pong_closes_connection() {
        let (url, server) = heartbeat_server(Timing {
            ping_interval: Duration::from_millis(20),
            ping_timeout: Duration::from_millis(50),
            ..Timing::default()
        })
        .await;
        let (mut client, _) = connect_async(url).await.unwrap();
        // Do not read, so no automatic pong is produced before the deadline.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(matches!(
            client.next().await.unwrap().unwrap(),
            Message::Ping(_)
        ));
        let frame = tokio::time::timeout(Duration::from_secs(2), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            matches!(frame, Message::Close(Some(frame)) if frame.reason == "WebSocket pong timed out")
        );
        client.flush().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn disabled_heartbeat_still_answers_client_ping() {
        let (url, server) = heartbeat_server(Timing {
            ping_interval: Duration::ZERO,
            ping_timeout: Duration::from_millis(20),
            ..Timing::default()
        })
        .await;
        let (mut client, _) = connect_async(url).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), client.next())
                .await
                .is_err()
        );
        client.send(Message::Ping(vec![1, 2, 3])).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), client.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Message::Pong(vec![1, 2, 3])
        );
        client.send(Message::Close(None)).await.unwrap();
        assert!(matches!(
            client.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        server.await.unwrap();
    }
}
