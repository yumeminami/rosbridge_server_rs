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

//! Per-connection FIFO command queues, served one command at a time round-robin.
use crate::outgoing::Output;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub struct QueueFull;
impl std::fmt::Display for QueueFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("client incoming queue full")
    }
}
impl std::error::Error for QueueFull {}

pub enum Command {
    Connect(u64, Output),
    Message(u64, Value),
    Disconnect(u64),
    Shutdown,
}
struct Client {
    output: Option<Output>,
    messages: VecDeque<Value>,
    disconnect: bool,
    scheduled: bool,
}
struct State {
    clients: HashMap<u64, Client>,
    ready: VecDeque<u64>,
    capacity: usize,
    closed: bool,
}
#[derive(Clone)]
pub struct Sender(Arc<Mutex<State>>);
pub struct Receiver(Arc<Mutex<State>>);

pub fn channel(capacity: usize) -> (Sender, Receiver) {
    assert!(capacity > 0);
    let state = Arc::new(Mutex::new(State {
        clients: HashMap::new(),
        ready: VecDeque::new(),
        capacity,
        closed: false,
    }));
    (Sender(state.clone()), Receiver(state))
}
impl State {
    fn schedule(&mut self, id: u64) {
        let client = self.clients.get_mut(&id).unwrap();
        if !client.scheduled {
            client.scheduled = true;
            self.ready.push_back(id);
        }
    }
}
impl Sender {
    pub fn connect(&self, id: u64, output: Output) -> anyhow::Result<()> {
        let mut s = self.0.lock().unwrap();
        anyhow::ensure!(!s.closed, "ROS command receiver closed");
        anyhow::ensure!(!s.clients.contains_key(&id), "duplicate connection");
        s.clients.insert(
            id,
            Client {
                output: Some(output),
                messages: VecDeque::new(),
                disconnect: false,
                scheduled: false,
            },
        );
        s.schedule(id);
        Ok(())
    }
    pub fn message(&self, id: u64, value: Value) -> anyhow::Result<()> {
        let mut s = self.0.lock().unwrap();
        anyhow::ensure!(!s.closed, "ROS command receiver closed");
        let capacity = s.capacity;
        let client = s
            .clients
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("connection is not registered"))?;
        anyhow::ensure!(!client.disconnect, "connection is closing");
        if client.messages.len() >= capacity {
            tracing::warn!(connection = id, capacity, "Client incoming queue full");
            anyhow::bail!(QueueFull);
        }
        client.messages.push_back(value);
        s.schedule(id);
        Ok(())
    }
    /// Lifecycle commands never compete with data for capacity.
    pub fn disconnect(&self, id: u64) {
        let mut s = self.0.lock().unwrap();
        if let Some(client) = s.clients.get_mut(&id) {
            client.messages.clear();
            client.disconnect = true;
            s.schedule(id);
        }
    }
    pub fn shutdown(&self) {
        let mut s = self.0.lock().unwrap();
        s.closed = true;
        s.clients.clear();
        s.ready.clear();
    }
}
impl Receiver {
    pub fn has_pending(&self) -> bool {
        let s = self.0.lock().unwrap();
        s.closed || !s.ready.is_empty()
    }
    pub fn try_recv(&self) -> Option<Command> {
        let mut s = self.0.lock().unwrap();
        if s.closed {
            return Some(Command::Shutdown);
        }
        let id = s.ready.pop_front()?;
        let client = s.clients.get_mut(&id).unwrap();
        client.scheduled = false;
        if client.disconnect {
            s.clients.remove(&id);
            return Some(Command::Disconnect(id));
        }
        let command = if let Some(output) = client.output.take() {
            Command::Connect(id, output)
        } else {
            Command::Message(id, client.messages.pop_front().unwrap())
        };
        if !client.messages.is_empty() {
            s.schedule(id);
        }
        Some(command)
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        let mut s = self.0.lock().unwrap();
        s.closed = true;
        s.clients.clear();
        s.ready.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outgoing;
    use serde_json::json;
    fn connect(tx: &Sender, rx: &Receiver, id: u64) {
        let (out, _) = outgoing::channel(1);
        tx.connect(id, out).unwrap();
        assert!(matches!(rx.try_recv(), Some(Command::Connect(found, _)) if found == id));
    }
    #[test]
    fn noisy_client_cannot_starve_other_clients_and_fifo_is_preserved() {
        let (tx, rx) = channel(128);
        connect(&tx, &rx, 1);
        connect(&tx, &rx, 2);
        for n in 0..128 {
            tx.message(1, json!(n)).unwrap();
        }
        tx.message(2, json!("urgent")).unwrap();
        assert!(matches!(rx.try_recv(), Some(Command::Message(1, v)) if v == 0));
        assert!(matches!(rx.try_recv(), Some(Command::Message(2, v)) if v == "urgent"));
        for n in 1..128 {
            assert!(matches!(rx.try_recv(), Some(Command::Message(1, v)) if v == n));
        }
        assert!(!rx.has_pending());
        tx.message(2, json!("again")).unwrap();
        assert!(matches!(rx.try_recv(), Some(Command::Message(2, _))));
    }
    #[test]
    fn capacity_is_per_client_and_disconnect_cannot_be_blocked() {
        let (tx, rx) = channel(1);
        connect(&tx, &rx, 1);
        connect(&tx, &rx, 2);
        tx.message(1, json!(1)).unwrap();
        assert!(tx.message(1, json!(2)).is_err());
        tx.message(2, json!(3)).unwrap();
        tx.disconnect(1);
        assert!(matches!(rx.try_recv(), Some(Command::Disconnect(1))));
        assert!(matches!(rx.try_recv(), Some(Command::Message(2, v)) if v == 3));
        assert!(tx.message(1, json!(4)).is_err());
    }
    #[test]
    fn connect_precedes_messages_and_shutdown_discards_backlog() {
        let (tx, rx) = channel(1);
        let (out, _) = outgoing::channel(1);
        tx.connect(1, out).unwrap();
        tx.message(1, json!(1)).unwrap();
        assert!(matches!(rx.try_recv(), Some(Command::Connect(1, _))));
        tx.shutdown();
        assert!(matches!(rx.try_recv(), Some(Command::Shutdown)));
        assert!(tx.message(1, json!(2)).is_err());
    }
    #[test]
    fn early_disconnect_and_worker_exit_release_pending_outputs() {
        let (tx, rx) = channel(1);
        let (out, mut peer) = outgoing::channel(1);
        tx.connect(1, out).unwrap();
        tx.disconnect(1);
        assert!(matches!(rx.try_recv(), Some(Command::Disconnect(1))));
        assert_eq!(
            peer.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        );
        drop(rx);
        assert!(tx.connect(2, outgoing::channel(1).0).is_err());
    }
}
