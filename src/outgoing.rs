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

//! Bounded per-client queues. A fragmented protocol message is always one batch.
use crate::encoding::{Pool, Request};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, mpsc::error::TryRecvError};
use tokio_tungstenite::tungstenite::Message;

pub const DEFAULT_BYTE_LIMIT: usize = 64 * 1024 * 1024;
pub const CONTROL_CAPACITY: usize = 16;

enum Payload {
    Frames(Vec<Message>),
    Encode(Box<Request>),
    #[cfg(test)]
    Probe(Box<dyn FnOnce() -> Vec<Message> + Send>),
}
impl Payload {
    fn bytes(&self) -> usize {
        match self {
            Self::Frames(frames) => frames.iter().map(Message::len).sum(),
            Self::Encode(request) => request.retained_bytes(),
            #[cfg(test)]
            Self::Probe(_) => 1,
        }
    }
    fn encode(self) -> anyhow::Result<Vec<Message>> {
        match self {
            Self::Frames(frames) => Ok(frames),
            Self::Encode(request) => request.encode(),
            #[cfg(test)]
            Self::Probe(run) => Ok(run()),
        }
    }
}
struct Batch {
    connection: u64,
    enqueued: Instant,
    topic: Option<String>,
    payload: Payload,
    bytes: usize,
}
struct State {
    topics: VecDeque<Batch>,
    control: VecDeque<Batch>,
    topic_bytes: usize,
    control_bytes: usize,
    capacity: usize,
    byte_limit: usize,
    sender_closed: bool,
    receiver_closed: bool,
    close_reason: Option<&'static str>,
    dropped: u64,
    last_warning: Option<Instant>,
}
struct Shared {
    state: Mutex<State>,
    ready: Notify,
    closed: Notify,
    pool: Pool,
}
pub struct Output(Arc<Shared>);
pub struct Receiver(Arc<Shared>);

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    Closed,
    ControlFull,
}

pub fn channel(capacity: usize) -> (Output, Receiver) {
    channel_with_limits(capacity, DEFAULT_BYTE_LIMIT)
}
pub fn channel_with_limits(capacity: usize, byte_limit: usize) -> (Output, Receiver) {
    channel_with_pool(capacity, byte_limit, Pool::default())
}
pub fn channel_with_pool(capacity: usize, byte_limit: usize, pool: Pool) -> (Output, Receiver) {
    assert!(capacity > 0 && byte_limit > 0);
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            topics: VecDeque::new(),
            control: VecDeque::new(),
            topic_bytes: 0,
            control_bytes: 0,
            capacity,
            byte_limit,
            sender_closed: false,
            receiver_closed: false,
            close_reason: None,
            dropped: 0,
            last_warning: None,
        }),
        ready: Notify::new(),
        closed: Notify::new(),
        pool,
    });
    (Output(shared.clone()), Receiver(shared))
}
impl Output {
    /// `topic_limit > 0` opts a topic into bounded oldest-first eviction.
    /// Control traffic has reserved capacity and is never silently discarded.
    pub fn send(
        &self,
        connection: u64,
        operation: &str,
        topic: Option<&str>,
        topic_limit: usize,
        frames: Vec<Message>,
    ) -> Result<(), SendError> {
        self.enqueue(
            connection,
            operation,
            topic,
            topic_limit,
            Payload::Frames(frames),
        )
    }
    pub fn send_request(
        &self,
        connection: u64,
        operation: &str,
        topic: Option<&str>,
        topic_limit: usize,
        request: Request,
    ) -> Result<(), SendError> {
        self.enqueue(
            connection,
            operation,
            topic,
            topic_limit,
            Payload::Encode(Box::new(request)),
        )
    }
    fn enqueue(
        &self,
        connection: u64,
        operation: &str,
        topic: Option<&str>,
        topic_limit: usize,
        payload: Payload,
    ) -> Result<(), SendError> {
        let bytes = payload.bytes();
        let mut s = self.0.state.lock().unwrap();
        if s.receiver_closed || s.sender_closed {
            return Err(SendError::Closed);
        }
        let mut dropped = 0;
        if let Some(topic) = topic {
            if bytes <= s.byte_limit && topic_limit > 0 {
                // Evict only this topic; never sacrifice another topic's backlog.
                while s
                    .topics
                    .iter()
                    .filter(|b| b.topic.as_deref() == Some(topic))
                    .count()
                    >= topic_limit
                    || s.topics.len() >= s.capacity
                    || bytes > s.byte_limit.saturating_sub(s.topic_bytes)
                {
                    let Some(index) = s
                        .topics
                        .iter()
                        .position(|b| b.topic.as_deref() == Some(topic))
                    else {
                        break;
                    };
                    let old = s.topics.remove(index).unwrap();
                    s.topic_bytes -= old.bytes;
                    dropped += 1;
                }
            }
            if s.topics.len() >= s.capacity || bytes > s.byte_limit.saturating_sub(s.topic_bytes) {
                dropped += 1;
            } else {
                s.topic_bytes += bytes;
                s.topics.push_back(Batch {
                    topic: Some(topic.into()),
                    payload,
                    connection,
                    enqueued: Instant::now(),
                    bytes,
                });
            }
            if dropped > 0 {
                s.dropped += dropped;
                if s.last_warning
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
                {
                    tracing::warn!(
                        connection,
                        operation,
                        topic,
                        dropped_batches = s.dropped,
                        capacity = s.capacity,
                        queued_bytes = s.topic_bytes,
                        byte_limit = s.byte_limit,
                        "Topic send queue full; dropping whole message batches"
                    );
                    s.last_warning = Some(Instant::now());
                }
            }
        } else {
            if s.control.len() >= CONTROL_CAPACITY
                || bytes > s.byte_limit.saturating_sub(s.control_bytes)
            {
                tracing::warn!(
                    connection,
                    operation,
                    capacity = CONTROL_CAPACITY,
                    queued_bytes = s.control_bytes,
                    byte_limit = s.byte_limit,
                    "Control send queue full; closing slow client"
                );
                s.close_reason = Some("control send queue full");
                s.sender_closed = true;
                s.topics.clear();
                s.control.clear();
                s.topic_bytes = 0;
                s.control_bytes = 0;
                drop(s);
                self.0.ready.notify_one();
                self.0.closed.notify_one();
                return Err(SendError::ControlFull);
            }
            s.control_bytes += bytes;
            s.control.push_back(Batch {
                topic: None,
                payload,
                connection,
                enqueued: Instant::now(),
                bytes,
            });
        }
        drop(s);
        self.0.ready.notify_one();
        Ok(())
    }
    pub fn is_closed(&self) -> bool {
        let s = self.0.state.lock().unwrap();
        s.receiver_closed || s.sender_closed
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let mut s = self.0.state.lock().unwrap();
        s.sender_closed = true;
        s.topics.clear();
        s.control.clear();
        s.topic_bytes = 0;
        s.control_bytes = 0;
        drop(s);
        self.0.ready.notify_one();
        self.0.closed.notify_one();
    }
}
impl Receiver {
    fn take(&mut self) -> Result<Batch, TryRecvError> {
        let mut s = self.0.state.lock().unwrap();
        if let Some(batch) = s.control.pop_front() {
            s.control_bytes -= batch.bytes;
            return Ok(batch);
        }
        if let Some(batch) = s.topics.pop_front() {
            s.topic_bytes -= batch.bytes;
            return Ok(batch);
        }
        Err(if s.sender_closed {
            TryRecvError::Disconnected
        } else {
            TryRecvError::Empty
        })
    }
    fn finish(
        &self,
        connection: u64,
        topic: bool,
        result: anyhow::Result<Vec<Message>>,
    ) -> Option<Vec<Message>> {
        let mut s = self.0.state.lock().unwrap();
        if s.sender_closed || s.receiver_closed {
            return None;
        }
        match result {
            Ok(frames) if frames.iter().map(Message::len).sum::<usize>() <= s.byte_limit => {
                Some(frames)
            }
            result => {
                let reason = match &result {
                    Ok(_) => "encoded batch exceeds byte limit",
                    Err(_) => "outbound encoding failed",
                };
                if topic {
                    s.dropped += 1;
                }
                if !topic
                    || s.last_warning
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
                {
                    tracing::warn!(connection, reason, error = ?result.err(), dropped_batches = s.dropped,
                        "Outbound batch rejected after encoding");
                    s.last_warning = Some(Instant::now());
                }
                if !topic {
                    s.close_reason = Some(reason);
                    s.sender_closed = true;
                    s.topics.clear();
                    s.control.clear();
                    s.topic_bytes = 0;
                    s.control_bytes = 0;
                }
                None
            }
        }
    }
    /// Synchronous consumer for protocol tests; sockets use `recv` and the pool.
    pub fn try_recv(&mut self) -> Result<Vec<Message>, TryRecvError> {
        loop {
            let batch = self.take()?;
            if let Some(frames) = self.finish(
                batch.connection,
                batch.topic.is_some(),
                batch.payload.encode(),
            ) {
                return Ok(frames);
            }
        }
    }
    pub async fn recv(&mut self) -> Option<Vec<Message>> {
        loop {
            let pending = {
                let s = self.0.state.lock().unwrap();
                if s.sender_closed {
                    return None;
                }
                !s.control.is_empty() || !s.topics.is_empty()
            };
            if !pending {
                self.0.ready.notified().await;
                continue;
            }
            // Waiters leave their data in the bounded queue, where newer topic
            // samples can replace it. Only permit holders spawn CPU work.
            let permit = tokio::select! {
                permit = self.0.pool.0.clone().acquire_owned() => permit.expect("encoding pool remains open"),
                _ = self.0.closed.notified() => return None,
            };
            let batch = match self.take() {
                Ok(batch) => batch,
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => continue,
            };
            let topic = batch.topic.is_some();
            let connection = batch.connection;
            let result = if !matches!(batch.payload, Payload::Frames(_)) {
                let shared = self.0.clone();
                tokio::task::spawn_blocking(move || {
                    let _permit = permit; // Cancellation cannot release it while encoding.
                    {
                        let state = shared.state.lock().unwrap();
                        anyhow::ensure!(
                            !state.sender_closed && !state.receiver_closed,
                            "encoding canceled"
                        );
                    }
                    let started = Instant::now();
                    let queue_wait_us = batch.enqueued.elapsed().as_micros() as u64;
                    let result = batch.payload.encode();
                    tracing::debug!(
                        connection = batch.connection,
                        input_bytes = batch.bytes,
                        queue_wait_us,
                        encode_us = started.elapsed().as_micros() as u64,
                        output_bytes = result
                            .as_ref()
                            .map(|frames| frames.iter().map(Message::len).sum::<usize>())
                            .unwrap_or(0),
                        "Outbound batch encoded"
                    );
                    result
                })
                .await
                .unwrap_or_else(|error| {
                    self.finish(
                        connection,
                        false,
                        Err(anyhow::anyhow!("encoding worker failed: {error}")),
                    );
                    Err(anyhow::anyhow!("encoding worker failed"))
                })
            } else {
                drop(permit);
                batch.payload.encode()
            };
            if let Some(frames) = self.finish(connection, topic, result) {
                return Some(frames);
            }
        }
    }
    pub fn close_reason(&self) -> Option<&'static str> {
        self.0.state.lock().unwrap().close_reason
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        let mut s = self.0.state.lock().unwrap();
        s.receiver_closed = true;
        s.topics.clear();
        s.control.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frames(s: &str) -> Vec<Message> {
        vec![Message::Text(s.into())]
    }

    #[test]
    fn topic_overflow_preserves_connection_and_control_reserve() {
        let (tx, mut rx) = channel(1);
        tx.send(1, "publish", Some("/x"), 0, frames("old")).unwrap();
        tx.send(1, "publish", Some("/x"), 0, frames("new")).unwrap();
        tx.send(1, "service_response", None, 0, frames("result"))
            .unwrap();
        assert_eq!(rx.try_recv().unwrap(), frames("result"));
        assert_eq!(rx.try_recv().unwrap(), frames("old"));
        assert!(!tx.is_closed());
        tx.send(1, "publish", Some("/x"), 0, frames("recovered"))
            .unwrap();
        assert_eq!(rx.try_recv().unwrap(), frames("recovered"));
    }

    #[test]
    fn latest_topic_policy_evicts_whole_batches_without_touching_other_topics() {
        let (tx, mut rx) = channel(2);
        tx.send(1, "publish", Some("/y"), 1, frames("y")).unwrap();
        tx.send(
            1,
            "publish",
            Some("/x"),
            1,
            vec![Message::Text("part1".into()), Message::Text("part2".into())],
        )
        .unwrap();
        tx.send(1, "publish", Some("/x"), 1, frames("latest"))
            .unwrap();
        assert_eq!(rx.try_recv().unwrap(), frames("y"));
        assert_eq!(rx.try_recv().unwrap(), frames("latest"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn byte_limits_and_closed_receiver() {
        let (tx, mut rx) = channel_with_limits(10, 4);
        tx.send(1, "publish", Some("/x"), 0, frames("1234"))
            .unwrap();
        tx.send(1, "publish", Some("/x"), 1, frames("12345"))
            .unwrap();
        assert_eq!(rx.try_recv().unwrap(), frames("1234"));
        tx.send(1, "publish", Some("/x"), 0, frames("1234"))
            .unwrap();
        tx.send(1, "publish", Some("/x"), 1, frames("new")).unwrap();
        assert_eq!(rx.try_recv().unwrap(), frames("new"));
        drop(rx);
        assert_eq!(
            tx.send(1, "publish", Some("/x"), 0, frames("x")),
            Err(SendError::Closed)
        );
    }

    #[tokio::test]
    async fn control_overflow_closes_immediately_with_reason() {
        let (tx, mut rx) = channel(1);
        for _ in 0..CONTROL_CAPACITY {
            tx.send(1, "action_result", None, 0, frames("result"))
                .unwrap();
        }
        assert_eq!(
            tx.send(1, "action_result", None, 0, frames("result")),
            Err(SendError::ControlFull)
        );
        assert_eq!(rx.recv().await, None);
        assert_eq!(rx.close_reason(), Some("control send queue full"));
    }

    #[tokio::test]
    async fn sender_drop_wakes_receiver() {
        let (tx, mut rx) = channel(1);
        let reader = tokio::spawn(async move { rx.recv().await });
        tokio::task::yield_now().await;
        drop(tx);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), reader)
                .await
                .unwrap()
                .unwrap(),
            None
        );
    }
    fn request(
        text: &str,
        compression: crate::wire::Compression,
        fragment: Option<usize>,
    ) -> Request {
        Request::new(
            serde_json::json!({"op":"publish","topic":"/x","msg":{"data":text}}),
            None,
            crate::wire::Options {
                compression,
                fragment,
                ..Default::default()
            },
            "fragment-test".into(),
        )
    }

    #[tokio::test]
    async fn waiting_for_pool_keeps_latest_policy_and_control_reserve_active() {
        let pool = Pool::new(1);
        let permit = pool.0.clone().acquire_owned().await.unwrap();
        let (tx, mut rx) = channel_with_pool(1, DEFAULT_BYTE_LIMIT, pool);
        tx.send_request(
            1,
            "publish",
            Some("/x"),
            1,
            request("old", crate::wire::Compression::Png, None),
        )
        .unwrap();
        let reader = tokio::spawn(async move {
            let control = rx.recv().await.unwrap();
            let topic = rx.recv().await.unwrap();
            (control, topic)
        });
        tokio::task::yield_now().await;
        // No payload was removed into an unbounded blocking-task backlog.
        assert_eq!(tx.0.state.lock().unwrap().topics.len(), 1);
        tx.send_request(
            1,
            "publish",
            Some("/x"),
            1,
            request("latest", crate::wire::Compression::None, None),
        )
        .unwrap();
        let control = Request::new(
            serde_json::json!({"op":"service_response","id":"call","values":42}),
            None,
            Default::default(),
            "control".into(),
        );
        tx.send_request(1, "service_response", None, 0, control)
            .unwrap();
        drop(permit);
        let (control, topic) = tokio::time::timeout(Duration::from_secs(2), reader)
            .await
            .unwrap()
            .unwrap();
        assert!(control[0].to_text().unwrap().contains("service_response"));
        assert!(topic[0].to_text().unwrap().contains("latest"));
        assert_eq!(tx.0.state.lock().unwrap().dropped, 1);
    }

    #[tokio::test]
    async fn pool_bounds_work_and_abort_does_not_release_running_permit() {
        let pool = Pool::new(1);
        let (tx, mut rx) = channel_with_pool(2, DEFAULT_BYTE_LIMIT, pool.clone());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let caller_thread = std::thread::current().id();
        tx.enqueue(
            1,
            "publish",
            Some("/x"),
            0,
            Payload::Probe(Box::new(move || {
                assert_ne!(std::thread::current().id(), caller_thread);
                started_tx.send(()).unwrap();
                let _ = release_rx.recv();
                frames("done")
            })),
        )
        .unwrap();
        let first = tokio::spawn(async move { rx.recv().await });
        tokio::time::timeout(Duration::from_secs(2), started_rx)
            .await
            .unwrap()
            .unwrap();
        first.abort();
        let _ = first.await;
        assert!(tx.is_closed());
        assert_eq!(pool.0.available_permits(), 0);
        let (second_tx, mut second_rx) = channel_with_pool(1, DEFAULT_BYTE_LIMIT, pool.clone());
        second_tx
            .send_request(
                2,
                "publish",
                Some("/x"),
                0,
                request("second", crate::wire::Compression::None, None),
            )
            .unwrap();
        let second = tokio::spawn(async move { second_rx.recv().await });
        tokio::task::yield_now().await;
        assert!(!second.is_finished());
        assert_eq!(second_tx.0.state.lock().unwrap().topics.len(), 1);
        release_tx.send(()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), second)
                .await
                .unwrap()
                .unwrap()
                .is_some()
        );
        assert_eq!(pool.0.available_permits(), 1);
    }

    #[tokio::test]
    async fn disconnect_cancels_queued_encoding_while_pool_is_busy() {
        let pool = Pool::new(1);
        let permit = pool.0.clone().acquire_owned().await.unwrap();
        let (tx, mut rx) = channel_with_pool(1, DEFAULT_BYTE_LIMIT, pool);
        tx.enqueue(
            1,
            "publish",
            Some("/x"),
            0,
            Payload::Probe(Box::new(|| panic!("canceled request encoded"))),
        )
        .unwrap();
        let reader = tokio::spawn(async move { rx.recv().await });
        tokio::task::yield_now().await;
        drop(tx);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), reader)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        drop(permit);
    }

    #[tokio::test]
    async fn encoding_compatibility_and_fifo_across_pool_jobs() {
        use crate::wire::{Compression, Decoder};
        let (tx, mut rx) = channel(8);
        for (compression, fragment) in [
            (Compression::None, Some(16)),
            (Compression::Png, None),
            (Compression::Cbor, None),
            (Compression::Raw, None),
        ] {
            let expected = request("中文 payload", compression.clone(), fragment)
                .encode()
                .unwrap();
            tx.send_request(
                1,
                "publish",
                Some("/x"),
                0,
                request("中文 payload", compression, fragment),
            )
            .unwrap();
            let actual = rx.recv().await.unwrap();
            assert_eq!(actual, expected);
            let mut decoder = Decoder::default();
            for frame in actual {
                decoder.decode(frame, 4096).unwrap();
            }
        }
        for text in ["first", "second", "third"] {
            tx.send_request(
                1,
                "publish",
                Some("/x"),
                0,
                request(text, Compression::None, None),
            )
            .unwrap();
        }
        for text in ["first", "second", "third"] {
            assert!(
                rx.recv().await.unwrap()[0]
                    .to_text()
                    .unwrap()
                    .contains(text)
            );
        }
    }

    #[tokio::test]
    async fn input_and_encoded_size_limits_are_both_enforced() {
        let input = request(&"x".repeat(1024), crate::wire::Compression::None, None);
        let (tx, mut rx) = channel_with_limits(1, input.retained_bytes() - 1);
        tx.send_request(1, "publish", Some("/x"), 0, input).unwrap();
        assert!(rx.try_recv().is_err());
        assert!(!tx.is_closed());
        let make = || request(&"x".repeat(1024), crate::wire::Compression::None, Some(1));
        let bytes = make().retained_bytes();
        let (tx, mut rx) = channel_with_limits(2, bytes);
        tx.send_request(1, "service_response", None, 0, make())
            .unwrap();
        assert_eq!(rx.recv().await, None);
        assert_eq!(rx.close_reason(), Some("encoded batch exceeds byte limit"));
    }

    #[tokio::test]
    async fn rejected_topic_is_not_encoded() {
        let (tx, mut rx) = channel(1);
        tx.send(1, "publish", Some("/x"), 0, frames("keep"))
            .unwrap();
        tx.enqueue(
            1,
            "publish",
            Some("/x"),
            0,
            Payload::Probe(Box::new(|| panic!("dropped request encoded"))),
        )
        .unwrap();
        assert_eq!(rx.recv().await.unwrap(), frames("keep"));
    }
    #[tokio::test]
    async fn encoding_error_drops_topic_but_closes_control_connection() {
        use crate::wire::Compression;
        let (tx, mut rx) = channel(2);
        tx.send_request(
            1,
            "publish",
            Some("/x"),
            0,
            request("中", Compression::None, Some(1)),
        )
        .unwrap();
        tx.send_request(
            1,
            "publish",
            Some("/x"),
            0,
            request("recovered", Compression::None, None),
        )
        .unwrap();
        assert!(
            rx.recv().await.unwrap()[0]
                .to_text()
                .unwrap()
                .contains("recovered")
        );
        assert!(!tx.is_closed());
        assert_eq!(tx.0.state.lock().unwrap().dropped, 1);
        tx.send_request(
            1,
            "service_response",
            None,
            0,
            request("中", Compression::None, Some(1)),
        )
        .unwrap();
        assert_eq!(rx.recv().await, None);
        assert_eq!(rx.close_reason(), Some("outbound encoding failed"));
        assert!(tx.is_closed());
    }
}
