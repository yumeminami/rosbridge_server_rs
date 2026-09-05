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

//! Topic registrations, shared subscriptions, and message delivery.

use super::{Bridge, ClientSubscription, Connection, Publisher, Subscription, id, name, uuid_key};
use crate::{
    backend::{Backend, Entity, Qos, RosMessage, type_name},
    wire::{Compression, Options},
};
use anyhow::{Context, Result, ensure};
use ciborium::Value as Cbor;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Instant,
};

impl<B: Backend> Bridge<B> {
    pub(super) fn advertise(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let topic = name(v, "topic")?;
        let registration = id(v)?;
        if let Some(p) = self.publishers.get_mut(&topic) {
            if let Some(t) = v.get("type") {
                ensure!(
                    type_name(t.as_str().context("invalid type")?, "msg")? == p.typ,
                    "topic type mismatch"
                );
            }
            p.owners.entry(owner).or_default().insert(registration);
            return Ok(());
        }
        let typ = self.resolve(v, "topic", "msg")?;
        let mut q = if let Some(q) = v.get("qos") {
            Qos::parse(q)?
        } else {
            Qos::publisher()
        };
        if v.get("qos").is_none() {
            if let Some(depth) = v.get("queue_size") {
                q.depth = depth
                    .as_u64()
                    .context("queue_size must be nonnegative")?
                    .try_into()?;
            }
            if let Some(latch) = v.get("latch") {
                q.durability = if latch.as_bool().context("latch must be a boolean")? {
                    1
                } else {
                    2
                };
            }
        }
        let entity = self.backend.publisher(&topic, &typ, q)?;
        self.publishers.insert(
            topic,
            Publisher {
                entity,
                typ,
                owners: HashMap::from([(owner, HashSet::from([registration]))]),
            },
        );
        Ok(())
    }

    pub(super) fn unadvertise(
        &mut self,
        owner: Connection,
        topic: &str,
        id: Option<String>,
    ) -> Result<()> {
        let p = self
            .publishers
            .get_mut(topic)
            .context("topic is not advertised")?;
        let ids = p
            .owners
            .get_mut(&owner)
            .context("topic is not advertised by this client")?;
        if let Some(id) = id {
            ensure!(ids.remove(&Some(id)), "unknown advertisement id");
        } else {
            ids.clear();
        }
        if ids.is_empty() {
            p.owners.remove(&owner);
        }
        if p.owners.is_empty() {
            let p = self.publishers.remove(topic).unwrap();
            self.backend.destroy(p.entity);
        }
        Ok(())
    }

    pub(super) fn subscribe(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let topic = name(v, "topic")?;
        let options = Options::parse(v)?;
        let typ = self.resolve(v, "topic", "msg")?;
        if let Some(s) = self.subscriptions.get(&topic) {
            ensure!(s.typ == typ, "topic type mismatch");
        } else {
            let q = if let Some(q) = v.get("qos") {
                Qos::parse(q)?
            } else {
                let mut q = Qos::subscriber();
                let pubs = self.backend.publisher_qos(&topic)?;
                if !pubs.is_empty() {
                    q.reliability = if pubs.iter().any(|q| q.reliability == 2) {
                        2
                    } else {
                        1
                    };
                    if pubs.iter().all(|q| q.durability == 1) {
                        q.durability = 1;
                        q.reliability = 1;
                    }
                }
                q
            };
            let entity = self.backend.subscription(&topic, &typ, q)?;
            self.subscriptions.insert(
                topic.clone(),
                Subscription {
                    transient_local: q.durability == 1,
                    retained: None,
                    entity,
                    typ,
                    clients: HashMap::new(),
                },
            );
        }
        let s = self.subscriptions.get_mut(&topic).unwrap();
        let new_client = !s.clients.contains_key(&owner);
        let c = s
            .clients
            .entry(owner)
            .or_insert_with(|| ClientSubscription {
                registrations: HashMap::new(),
                last: None,
                pending: VecDeque::new(),
            });
        c.registrations.insert(id(v)?, options.clone());
        tracing::info!(connection = owner, %topic, "Subscribed");
        let retained = new_client.then(|| s.retained.clone()).flatten();
        if let Some(message) = retained {
            c.last = Some(Instant::now());
            self.deliver_topic(owner, &topic, message, &options);
        }
        Ok(())
    }

    pub(super) fn unsubscribe(
        &mut self,
        owner: Connection,
        topic: &str,
        id: Option<String>,
    ) -> Result<()> {
        let s = self
            .subscriptions
            .get_mut(topic)
            .context("topic is not subscribed")?;
        let c = s
            .clients
            .get_mut(&owner)
            .context("topic is not subscribed by this client")?;
        if let Some(id) = id {
            ensure!(
                c.registrations.remove(&Some(id)).is_some(),
                "unknown subscription id"
            );
        } else {
            c.registrations.clear();
        }
        if c.registrations.is_empty() {
            s.clients.remove(&owner);
        }
        if s.clients.is_empty() {
            let s = self.subscriptions.remove(topic).unwrap();
            self.backend.destroy(s.entity);
        }
        tracing::info!(connection = owner, %topic, "Unsubscribed");
        Ok(())
    }

    pub(super) fn topic_message(&mut self, entity: Entity, message: RosMessage) -> Result<()> {
        if let Some(g) = self.goals.values().find(|g| g.feedback_sub == entity) {
            if g.feedback && uuid_key(&g.uuid)? == uuid_key(&message.json["goal_id"])? {
                let owner = g.owner;
                let id = g.id.clone();
                let options = g.options.clone();
                let action = g.action.clone();
                let response = json!({
                    "op": "action_feedback",
                    "action": action,
                    "values": message.json["feedback"],
                });
                self.send(owner, response, &id, &options, None);
            }
            return Ok(());
        }
        let mut deliveries = Vec::new();
        if let Some((topic, s)) = self
            .subscriptions
            .iter_mut()
            .find(|(_, s)| s.entity == entity)
        {
            // Shared native subscriptions receive DDS history only once. Replay the
            // latest transient-local sample when another WebSocket client joins.
            if s.transient_local {
                s.retained = Some(message.clone());
            }
            for (&owner, c) in &mut s.clients {
                let options = c.options();
                if c.pending.is_empty() && c.last.is_none_or(|t| t.elapsed() >= options.throttle) {
                    c.last = Some(Instant::now());
                    deliveries.push((owner, topic.clone(), message.clone(), options));
                } else if options.queue > 0 {
                    while c.pending.len() >= options.queue {
                        c.pending.pop_front();
                    }
                    c.pending.push_back(message.clone());
                }
            }
        }
        for (owner, topic, message, options) in deliveries {
            self.deliver_topic(owner, &topic, message, &options);
        }
        Ok(())
    }

    pub(super) fn deliver_topic(
        &mut self,
        owner: Connection,
        topic: &str,
        msg: RosMessage,
        options: &Options,
    ) {
        let value = json!({"op":"publish","topic":topic,"msg":msg.json});
        let body = if options.compression == Compression::Raw {
            Cbor::Map(vec![
                (Cbor::Text("bytes".into()), Cbor::Bytes(msg.raw)),
                (Cbor::Text("secs".into()), Cbor::Integer(msg.stamp.0.into())),
                (
                    Cbor::Text("nsecs".into()),
                    Cbor::Integer(msg.stamp.1.into()),
                ),
            ])
        } else {
            msg.cbor
        };
        let binary = Cbor::Map(vec![
            (Cbor::Text("op".into()), Cbor::Text("publish".into())),
            (Cbor::Text("topic".into()), Cbor::Text(topic.into())),
            (Cbor::Text("msg".into()), body),
        ]);
        self.send(owner, value, &None, options, Some(binary));
    }
}
