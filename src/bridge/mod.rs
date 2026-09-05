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

//! Protocol dispatch and connection-owned ROS resources.

mod actions;
mod services;
mod topics;

use crate::{
    backend::{Backend, Entity, Event, RosMessage, type_name},
    wire::{self, Compression, Options},
};
use anyhow::{Context, Result, bail, ensure};
use ciborium::Value as Cbor;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
pub type Connection = u64;
pub type Output = mpsc::Sender<Vec<Message>>;
struct Publisher {
    entity: Entity,
    typ: String,
    owners: HashMap<Connection, HashSet<Option<String>>>,
}

struct Subscription {
    transient_local: bool,
    retained: Option<RosMessage>,
    entity: Entity,
    typ: String,
    clients: HashMap<Connection, ClientSubscription>,
}

struct ClientSubscription {
    registrations: HashMap<Option<String>, Options>,
    last: Option<Instant>,
    pending: VecDeque<RosMessage>,
}

impl ClientSubscription {
    fn options(&self) -> Options {
        let mut result = self
            .registrations
            .values()
            .next()
            .cloned()
            .unwrap_or_default();
        result.throttle = self
            .registrations
            .values()
            .map(|o| o.throttle)
            .min()
            .unwrap_or_default();
        result.queue = self
            .registrations
            .values()
            .map(|o| o.queue)
            .min()
            .unwrap_or(0);
        result.fragment = self.registrations.values().filter_map(|o| o.fragment).min();
        // Match Python rosbridge's precedence for registrations sharing a topic.
        result.compression = [Compression::Raw, Compression::Cbor, Compression::Png]
            .into_iter()
            .find(|compression| {
                self.registrations
                    .values()
                    .any(|o| o.compression == *compression)
            })
            .unwrap_or_default();
        result
    }
}

struct Service {
    next_request: u64,
    owner: Connection,
    entity: Entity,
    typ: String,
}

struct Call {
    owner: Connection,
    id: Option<String>,
    service: String,
    options: Options,
    expires: Instant,
}

struct ExternalRequest {
    owner: Connection,
    entity: Entity,
    request: u64,
    service: String,
    expires: Instant,
}

struct ClientGoal {
    owner: Connection,
    id: Option<String>,
    action: String,
    uuid: Value,
    feedback: bool,
    options: Options,
    send: Entity,
    result: Entity,
    cancel: Entity,
    feedback_sub: Entity,
    status_sub: Entity,
    accepted: bool,
    acceptance_deadline: Instant,
    cancelling: bool,
}

struct ActionServer {
    owner: Connection,
    advertised_type: String,
    typ: String,
    send: Entity,
    cancel: Entity,
    result: Entity,
    feedback: Entity,
    status: Entity,
}

struct ServerGoal {
    owner: Connection,
    action: String,
    id: String,
    uuid: Value,
    status: i64,
    stamp: Value,
    result: Option<Value>,
    requests: Vec<(Entity, u64)>,
    completed: Option<Instant>,
}

#[derive(Clone)]
enum ActionCall {
    Send(String),
    Result(String),
    Cancel(String),
}

pub struct Bridge<B: Backend> {
    pub backend: B,
    outputs: HashMap<Connection, Output>,
    publishers: HashMap<String, Publisher>,
    subscriptions: HashMap<String, Subscription>,
    services: HashMap<String, Service>,
    calls: HashMap<(Entity, i64), Call>,
    external: HashMap<String, ExternalRequest>,
    goals: HashMap<String, ClientGoal>,
    action_calls: HashMap<(Entity, i64), ActionCall>,
    action_servers: HashMap<String, ActionServer>,
    server_goals: HashMap<String, ServerGoal>,
    next: u64,
    timeout: Duration,
    failed: HashSet<Connection>,
}

fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{key} must be a nonempty string"))
}

fn id(v: &Value) -> Result<Option<String>> {
    v.get("id")
        .map(|v| v.as_str().map(str::to_owned).context("id must be a string"))
        .transpose()
}

fn name(v: &Value, key: &str) -> Result<String> {
    let n = required(v, key)?;
    ensure!(
        !n.contains('~')
            && n.split('/')
                .filter(|s| !s.is_empty())
                .all(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')),
        "invalid ROS name"
    );
    Ok(if n.starts_with('/') {
        n.to_owned()
    } else {
        format!("/{n}")
    })
}

impl<B: Backend> Bridge<B> {
    pub fn new(backend: B, timeout: Duration) -> Self {
        Self {
            backend,
            outputs: HashMap::new(),
            publishers: HashMap::new(),
            subscriptions: HashMap::new(),
            services: HashMap::new(),
            calls: HashMap::new(),
            external: HashMap::new(),
            goals: HashMap::new(),
            action_calls: HashMap::new(),
            action_servers: HashMap::new(),
            server_goals: HashMap::new(),
            next: 0,
            timeout,
            failed: HashSet::new(),
        }
    }

    pub fn connect(&mut self, connection: Connection, output: Output) {
        self.outputs.insert(connection, output);
    }

    fn unique(&mut self) -> String {
        self.next += 1;
        format!("rosbridge:{}", self.next)
    }

    fn send(
        &mut self,
        owner: Connection,
        mut value: Value,
        request_id: &Option<String>,
        options: &Options,
        binary: Option<Cbor>,
    ) {
        if let Some(id) = request_id {
            value["id"] = json!(id);
        }
        let key = self.unique();
        match wire::encode(&value, binary, options, &key) {
            Ok(frames) => {
                if let Some(output) = self.outputs.get(&owner)
                    && output.try_send(frames).is_err()
                {
                    self.failed.insert(owner);
                }
            }
            Err(e) => {
                tracing::warn!("encode response: {e}");
            }
        }
    }

    pub fn error(&mut self, owner: Connection, v: &Value, error: &anyhow::Error) {
        let operation = v["op"].as_str().unwrap_or("");
        let request_id = id(v).ok().flatten();
        let response = match operation {
            "call_service" => json!({
                "op": "service_response",
                "service": v["service"],
                "result": false,
                "values": error.to_string(),
            }),
            "send_action_goal" => json!({
                "op": "action_result",
                "action": v["action"],
                "result": false,
                "status": 6,
                "values": error.to_string(),
            }),
            _ => json!({
                "op": "status",
                "level": "error",
                "msg": error.to_string(),
            }),
        };
        self.send(owner, response, &request_id, &Options::default(), None);
    }

    pub fn command(&mut self, owner: Connection, v: Value) {
        if let Err(e) = self.execute(owner, &v) {
            tracing::warn!(connection = owner, "{e:#}");
            self.error(owner, &v, &e);
        }
    }

    fn execute(&mut self, owner: Connection, v: &Value) -> Result<()> {
        id(v)?;
        match required(v, "op")? {
            "advertise" => {
                self.advertise(owner, v)?;
            }
            "unadvertise" => self.unadvertise(owner, &name(v, "topic")?, id(v)?)?,
            "publish" => {
                let topic = name(v, "topic")?;
                ensure!(v["msg"].is_object(), "msg must be an object");
                self.advertise(owner, v)?;
                self.backend
                    .publish(self.publishers[&topic].entity, &v["msg"])?;
            }
            "subscribe" => self.subscribe(owner, v)?,
            "unsubscribe" => self.unsubscribe(owner, &name(v, "topic")?, id(v)?)?,
            "call_service" => self.call_service(owner, v)?,
            "advertise_service" => self.advertise_service(owner, v)?,
            "unadvertise_service" => self.remove_service(owner, &name(v, "service")?)?,
            "service_response" => self.service_response(owner, v)?,
            "send_action_goal" => self.send_goal(owner, v)?,
            "cancel_action_goal" => self.cancel_goal(owner, v)?,
            "advertise_action" => self.advertise_action(owner, v)?,
            "unadvertise_action" => self.remove_action(owner, &name(v, "action")?)?,
            "action_feedback" => self.action_feedback(owner, v)?,
            "action_result" => self.action_result(owner, v)?,
            other => bail!("unknown operation: {other}"),
        }
        Ok(())
    }

    fn resolve(&mut self, v: &Value, key: &str, kind: &str) -> Result<String> {
        let graph = if kind == "msg" {
            self.backend.topics()?
        } else {
            self.backend.services()?
        };
        let topic = name(v, key)?;
        if let Some(typ) = v.get("type") {
            let typ = type_name(typ.as_str().context("type must be a string")?, kind)?;
            if let Some(types) = graph.get(&topic) {
                ensure!(
                    types.iter().all(|t| t == &typ),
                    "{topic} has another type: requested {typ}, ROS graph {types:?}"
                );
            }
            return Ok(typ);
        }
        let types = graph
            .get(&topic)
            .context("type is required when interface is absent from ROS graph")?;
        ensure!(types.len() == 1, "interface has ambiguous types");
        Ok(types[0].clone())
    }

    fn response(&mut self, entity: Entity, sequence: i64, values: Value) -> Result<()> {
        if let Some(call) = self.calls.remove(&(entity, sequence)) {
            let response = json!({
                "op": "service_response",
                "service": call.service,
                "result": true,
                "values": values,
            });
            self.send(call.owner, response, &call.id, &call.options, None);
            self.backend.destroy(entity);
            return Ok(());
        }
        if let Some(call) = self.action_calls.remove(&(entity, sequence)) {
            match call {
                ActionCall::Send(key) => {
                    if values["accepted"] != true {
                        self.finish_goal(&key, json!("goal rejected"), 6, false);
                    } else if let Some(g) = self.goals.get_mut(&key) {
                        g.accepted = true;
                        let seq = self.backend.request(g.result, &json!({"goal_id":g.uuid}))?;
                        self.action_calls
                            .insert((g.result, seq), ActionCall::Result(key));
                    }
                }
                ActionCall::Result(key) => {
                    let status = values["status"].as_i64().unwrap_or(0);
                    self.finish_goal(&key, values["result"].clone(), status, status == 4);
                }
                ActionCall::Cancel(key) => {
                    if values["return_code"] != 0
                        && let Some(g) = self.goals.get_mut(&key)
                    {
                        g.cancelling = false;
                        let owner = g.owner;
                        let id = g.id.clone();
                        let response = json!({
                            "op": "status",
                            "level": "error",
                            "msg": "action cancellation rejected",
                        });
                        self.send(owner, response, &id, &Options::default(), None);
                    }
                }
            }
        }
        Ok(())
    }

    /// Bound the event wait by queued deliveries and response deadlines.
    pub fn next_wakeup(&self) -> Duration {
        let now = Instant::now();
        let mut wait = Duration::from_millis(100);
        for call in self.calls.values() {
            wait = wait.min(call.expires.saturating_duration_since(now));
        }
        for request in self.external.values() {
            wait = wait.min(request.expires.saturating_duration_since(now));
        }
        for goal in self.goals.values().filter(|goal| !goal.accepted) {
            wait = wait.min(goal.acceptance_deadline.saturating_duration_since(now));
        }
        for client in self.subscriptions.values().flat_map(|s| s.clients.values()) {
            if !client.pending.is_empty() {
                let remaining = client.last.map_or(Duration::ZERO, |last| {
                    client
                        .options()
                        .throttle
                        .saturating_sub(now.duration_since(last))
                });
                wait = wait.min(remaining);
            }
        }
        wait
    }

    pub fn tick(&mut self) -> Result<()> {
        for event in self.backend.poll()? {
            let result = match event {
                Event::Message(entity, msg) => self.topic_message(entity, msg),
                Event::Response {
                    entity,
                    sequence,
                    values,
                } => self.response(entity, sequence, values),
                Event::Request {
                    entity,
                    request,
                    args,
                } => {
                    let result = self.service_request(entity, request, args);
                    if result.is_err() {
                        self.backend.discard_request(entity, request);
                    }
                    result
                }
            };
            if let Err(e) = result {
                tracing::warn!("ROS event: {e:#}");
            }
        }
        let now = Instant::now();
        let expired_goals: Vec<_> = self
            .goals
            .iter()
            .filter(|(_, g)| !g.accepted && g.acceptance_deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired_goals {
            self.finish_goal(&key, json!({}), 6, false);
        }
        let mut deliveries = Vec::new();
        for (topic, s) in &mut self.subscriptions {
            for (&owner, c) in &mut s.clients {
                let options = c.options();
                if c.last
                    .is_none_or(|t| now.duration_since(t) >= options.throttle)
                    && let Some(msg) = c.pending.pop_front()
                {
                    c.last = Some(now);
                    deliveries.push((owner, topic.clone(), msg, options));
                }
            }
        }
        for (owner, topic, msg, options) in deliveries {
            self.deliver_topic(owner, &topic, msg, &options);
        }
        let expired: Vec<_> = self
            .calls
            .iter()
            .filter(|(_, c)| c.expires <= now)
            .map(|(k, _)| *k)
            .collect();
        for key in expired {
            let c = self.calls.remove(&key).unwrap();
            let response = json!({
                "op": "service_response",
                "service": c.service,
                "values": "Timeout exceeded while waiting for service response",
                "result": false,
            });
            self.send(c.owner, response, &c.id, &c.options, None);
            self.backend.destroy(key.0);
        }
        let expired: Vec<_> = self
            .external
            .iter()
            .filter(|(_, r)| r.expires <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            let r = self.external.remove(&id).unwrap();
            self.backend.discard_request(r.entity, r.request);
        }
        self.server_goals.retain(|_, g| {
            g.completed
                .is_none_or(|t| t.elapsed() < Duration::from_secs(60))
        });
        let failed: Vec<_> = self.failed.drain().collect();
        for id in failed {
            self.disconnect(id);
        }
        let disconnected: Vec<_> = self
            .outputs
            .iter()
            .filter(|(_, o)| o.is_closed())
            .map(|(id, _)| *id)
            .collect();
        for id in disconnected {
            self.disconnect(id);
        }
        Ok(())
    }

    pub fn disconnect(&mut self, owner: Connection) {
        self.outputs.remove(&owner);
        let topics: Vec<_> = self
            .publishers
            .iter()
            .filter(|(_, p)| p.owners.contains_key(&owner))
            .map(|(n, _)| n.clone())
            .collect();
        for n in topics {
            let _ = self.unadvertise(owner, &n, None);
        }
        let topics: Vec<_> = self
            .subscriptions
            .iter()
            .filter(|(_, p)| p.clients.contains_key(&owner))
            .map(|(n, _)| n.clone())
            .collect();
        for n in topics {
            let _ = self.unsubscribe(owner, &n, None);
        }
        let services: Vec<_> = self
            .services
            .iter()
            .filter(|(_, s)| s.owner == owner)
            .map(|(n, _)| n.clone())
            .collect();
        for n in services {
            let _ = self.remove_service(owner, &n);
        }
        let actions: Vec<_> = self
            .action_servers
            .iter()
            .filter(|(_, s)| s.owner == owner)
            .map(|(n, _)| n.clone())
            .collect();
        for n in actions {
            let _ = self.remove_action(owner, &n);
        }
        let goals: Vec<_> = self
            .goals
            .iter()
            .filter(|(_, g)| g.owner == owner)
            .map(|(k, _)| k.clone())
            .collect();
        for key in goals {
            if let Some(g) = self.goals.get(&key)
                && g.accepted
            {
                let _ = self.backend.request(
                    g.cancel,
                    &json!({"goal_info":{"goal_id":g.uuid,"stamp":{"sec":0,"nanosec":0}}}),
                );
            }
            self.finish_goal(&key, json!("client disconnected"), 6, false);
        }
        let calls: Vec<_> = self
            .calls
            .iter()
            .filter(|(_, c)| c.owner == owner)
            .map(|(k, _)| *k)
            .collect();
        for key in calls {
            self.calls.remove(&key);
            self.backend.destroy(key.0);
        }
        self.external.retain(|_, r| r.owner != owner);
    }

    pub fn shutdown(&mut self) {
        let owners: Vec<_> = self.outputs.keys().copied().collect();
        for owner in owners {
            self.disconnect(owner);
        }
    }
}

fn uuid_key(v: &Value) -> Result<String> {
    use base64::Engine;
    let bytes = if let Some(s) = v["uuid"].as_str() {
        base64::engine::general_purpose::STANDARD.decode(s)?
    } else {
        v["uuid"]
            .as_array()
            .context("missing goal UUID")?
            .iter()
            .map(|v| Ok(u8::try_from(v.as_u64().context("invalid UUID byte")?)?))
            .collect::<Result<Vec<_>>>()?
    };
    ensure!(bytes.len() == 16, "goal UUID must be 16 bytes");
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
