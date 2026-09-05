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

//! ROS Action goal lifecycle and WebSocket action servers.

use super::{
    ActionCall, ActionServer, Bridge, ClientGoal, Connection, ServerGoal, id, name, required,
    uuid_key,
};
use crate::{
    backend::{Backend, Entity, Qos, type_name},
    wire::Options,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::time::Instant;
use uuid::Uuid;

impl<B: Backend> Bridge<B> {
    pub(super) fn send_goal(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let action = name(v, "action")?;
        let typ = type_name(required(v, "action_type")?, "action")?;
        let request_id = id(v)?;
        let options = Options::parse(v)?;
        ensure!(
            !self
                .goals
                .values()
                .any(|g| g.owner == owner && g.action == action && g.id == request_id),
            "goal id is already active"
        );
        let feedback = v
            .get("feedback")
            .map(|v| v.as_bool().context("feedback must be boolean"))
            .transpose()?
            .unwrap_or(false);
        let uuid = json!({"uuid":Uuid::new_v4().as_bytes().to_vec()});
        let key = uuid_key(&uuid)?;
        let mut created = Vec::new();
        let result = (|| -> Result<()> {
            let send = self.backend.client(
                &format!("{action}/_action/send_goal"),
                &format!("{typ}_SendGoal"),
            )?;
            created.push(send);
            let result = self.backend.client(
                &format!("{action}/_action/get_result"),
                &format!("{typ}_GetResult"),
            )?;
            created.push(result);
            let cancel = self.backend.client(
                &format!("{action}/_action/cancel_goal"),
                "action_msgs/srv/CancelGoal",
            )?;
            created.push(cancel);
            let feedback_sub = self.backend.subscription(
                &format!("{action}/_action/feedback"),
                &format!("{typ}_FeedbackMessage"),
                Qos::service(),
            )?;
            created.push(feedback_sub);
            let mut q = Qos::service();
            q.durability = 1;
            let status_sub = self.backend.subscription(
                &format!("{action}/_action/status"),
                "action_msgs/msg/GoalStatusArray",
                q,
            )?;
            created.push(status_sub);
            let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
            let sequence = self
                .backend
                .request(send, &json!({"goal_id":uuid,"goal":args}))?;
            self.action_calls
                .insert((send, sequence), ActionCall::Send(key.clone()));
            self.goals.insert(
                key.clone(),
                ClientGoal {
                    owner,
                    id: request_id,
                    action,
                    uuid,
                    feedback,
                    options,
                    send,
                    result,
                    cancel,
                    feedback_sub,
                    status_sub,
                    accepted: false,
                    acceptance_deadline: Instant::now() + self.timeout,
                    cancelling: false,
                },
            );
            Ok(())
        })();
        if result.is_err() {
            for entity in created {
                self.backend.destroy(entity);
            }
        }
        result
    }

    pub(super) fn cancel_goal(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let action = name(v, "action")?;
        let id = required(v, "id")?;
        let (key, g) = self
            .goals
            .iter_mut()
            .find(|(_, g)| g.owner == owner && g.action == action && g.id.as_deref() == Some(id))
            .context("unknown active goal")?;
        ensure!(!g.cancelling, "goal cancellation is already in progress");
        ensure!(g.accepted, "goal is not accepted yet");
        let sequence = self.backend.request(
            g.cancel,
            &json!({"goal_info":{"goal_id":g.uuid,"stamp":{"sec":0,"nanosec":0}}}),
        )?;
        g.cancelling = true;
        self.action_calls
            .insert((g.cancel, sequence), ActionCall::Cancel(key.clone()));
        Ok(())
    }

    pub(super) fn finish_goal(&mut self, key: &str, values: Value, status: i64, success: bool) {
        if let Some(g) = self.goals.remove(key) {
            let response = json!({
                "op": "action_result",
                "action": g.action,
                "values": values,
                "status": status,
                "result": success,
            });
            self.send(g.owner, response, &g.id, &g.options, None);
            for e in [g.send, g.result, g.cancel, g.feedback_sub, g.status_sub] {
                self.backend.destroy(e);
            }
            self.action_calls.retain(|_, c| match c {
                ActionCall::Send(k) | ActionCall::Result(k) | ActionCall::Cancel(k) => k != key,
            });
        }
    }

    pub(super) fn advertise_action(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let action = name(v, "action")?;
        let typ = type_name(required(v, "type")?, "action")?;
        if let Some(a) = self.action_servers.get(&action) {
            ensure!(a.owner == owner, "action is advertised by another client");
            if a.typ == typ {
                return Ok(());
            }
            self.remove_action(owner, &action)?;
        }
        let mut created = Vec::new();
        let result = (|| -> Result<()> {
            let send = self.backend.service(
                &format!("{action}/_action/send_goal"),
                &format!("{typ}_SendGoal"),
            )?;
            created.push(send);
            let result = self.backend.service(
                &format!("{action}/_action/get_result"),
                &format!("{typ}_GetResult"),
            )?;
            created.push(result);
            let cancel = self.backend.service(
                &format!("{action}/_action/cancel_goal"),
                "action_msgs/srv/CancelGoal",
            )?;
            created.push(cancel);
            let feedback = self.backend.publisher(
                &format!("{action}/_action/feedback"),
                &format!("{typ}_FeedbackMessage"),
                Qos::service(),
            )?;
            created.push(feedback);
            let mut q = Qos::service();
            q.durability = 1;
            q.depth = 1;
            let status = self.backend.publisher(
                &format!("{action}/_action/status"),
                "action_msgs/msg/GoalStatusArray",
                q,
            )?;
            created.push(status);
            self.action_servers.insert(
                action,
                ActionServer {
                    owner,
                    advertised_type: required(v, "type")?.to_owned(),
                    typ,
                    send,
                    cancel,
                    result,
                    feedback,
                    status,
                },
            );
            Ok(())
        })();
        if result.is_err() {
            for e in created {
                self.backend.destroy(e);
            }
        }
        result
    }

    pub(super) fn remove_action(&mut self, owner: Connection, action: &str) -> Result<()> {
        ensure!(
            self.action_servers
                .get(action)
                .is_some_and(|a| a.owner == owner),
            "action is not advertised by this client"
        );
        let keys: Vec<_> = self
            .server_goals
            .iter()
            .filter(|(_, g)| g.action == action)
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            if let Some(g) = self.server_goals.remove(&key) {
                for (entity, request) in g.requests {
                    let _ = self
                        .backend
                        .respond(entity, request, &json!({"status":6,"result":{}}));
                }
            }
        }
        let a = self.action_servers.remove(action).unwrap();
        for e in [a.send, a.result, a.cancel, a.feedback, a.status] {
            self.backend.destroy(e);
        }
        Ok(())
    }

    pub(super) fn action_feedback(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let id = required(v, "id")?;
        let action = name(v, "action")?;
        let g = self
            .server_goals
            .values()
            .find(|g| g.owner == owner && g.id == id && g.action == action)
            .context("unknown goal")?;
        ensure!(g.result.is_none(), "goal has completed");
        ensure!(v["values"].is_object(), "feedback values must be an object");
        self.backend.publish(
            self.action_servers[&action].feedback,
            &json!({"goal_id":g.uuid,"feedback":v["values"]}),
        )
    }

    pub(super) fn action_result(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let id = required(v, "id")?;
        let action = name(v, "action")?;
        let g = self
            .server_goals
            .values_mut()
            .find(|g| g.owner == owner && g.id == id && g.action == action)
            .context("unknown goal")?;
        ensure!(g.result.is_none(), "goal has completed");
        let status = v["status"].as_i64().context("status must be an integer")?;
        ensure!(
            (4..=6).contains(&status),
            "result requires terminal action status (4,5,6)"
        );
        let success = v["result"].as_bool().context("result must be boolean")?;
        let values = if success {
            ensure!(v["values"].is_object(), "values must be an object");
            v["values"].clone()
        } else {
            json!({})
        };
        let response = json!({"status":status,"result":values});
        // A result may arrive before any GetResult request. Validate before caching it.
        self.backend
            .validate_response(self.action_servers[&action].result, &response)?;
        while let Some(&(entity, request)) = g.requests.last() {
            self.backend.respond(entity, request, &response)?;
            // A successful response consumes its native request, even if a later send fails.
            g.requests.pop();
        }
        g.status = status;
        g.result = Some(values);
        g.completed = Some(Instant::now());
        self.publish_status(&action)
    }

    pub(super) fn publish_status(&mut self, action: &str) -> Result<()> {
        let list: Vec<_> = self
            .server_goals
            .values()
            .filter(|g| g.action == action)
            .map(|g| json!({"goal_info":{"goal_id":g.uuid,"stamp":g.stamp},"status":g.status}))
            .collect();
        self.backend.publish(
            self.action_servers[action].status,
            &json!({"status_list":list}),
        )
    }

    pub(super) fn action_request(
        &mut self,
        entity: Entity,
        request: u64,
        args: Value,
    ) -> Result<bool> {
        let Some((action, a)) = self
            .action_servers
            .iter()
            .find(|(_, a)| [a.send, a.cancel, a.result].contains(&entity))
        else {
            return Ok(false);
        };
        let action = action.clone();
        let owner = a.owner;
        let typ = a.advertised_type.clone();
        let send = a.send;
        let result = a.result;
        if entity == send {
            let uuid = args["goal_id"].clone();
            let key = format!("{action}:{}", uuid_key(&uuid)?);
            let now = self.backend.now();
            let stamp = json!({"sec":now.0,"nanosec":now.1});
            if self.server_goals.contains_key(&key) {
                self.backend
                    .respond(entity, request, &json!({"accepted":false,"stamp":stamp}))?;
                return Ok(true);
            }
            let id = self.unique();
            self.backend
                .respond(entity, request, &json!({"accepted":true,"stamp":stamp}))?;
            self.server_goals.insert(
                key,
                ServerGoal {
                    owner,
                    action: action.clone(),
                    id: id.clone(),
                    uuid,
                    status: 2,
                    stamp,
                    result: None,
                    requests: Vec::new(),
                    completed: None,
                },
            );
            let response = json!({
                "op": "send_action_goal",
                "action": action,
                "action_type": typ,
                "args": args["goal"],
                "feedback": true,
            });
            self.send(owner, response, &Some(id), &Options::default(), None);
            self.publish_status(&action)?;
        } else if entity == result {
            let key = format!("{action}:{}", uuid_key(&args["goal_id"])?);
            if let Some(g) = self.server_goals.get_mut(&key) {
                if let Some(value) = &g.result {
                    self.backend.respond(
                        entity,
                        request,
                        &json!({"status":g.status,"result":value}),
                    )?;
                } else {
                    g.requests.push((entity, request));
                }
            } else {
                self.backend
                    .respond(entity, request, &json!({"status":0,"result":{}}))?;
            }
        } else {
            let requested = uuid_key(&args["goal_info"]["goal_id"])?;
            let all = requested == "00000000000000000000000000000000";
            let stamp = (
                args["goal_info"]["stamp"]["sec"].as_i64().unwrap_or(0),
                args["goal_info"]["stamp"]["nanosec"].as_u64().unwrap_or(0),
            );
            let mut cancelling = Vec::new();
            let mut notify = Vec::new();
            let mut known = false;
            let mut terminal = false;
            for g in self
                .server_goals
                .values_mut()
                .filter(|g| g.action == action)
            {
                let exact = uuid_key(&g.uuid)? == requested;
                if exact {
                    known = true;
                    terminal = g.result.is_some();
                }
                let before = (
                    g.stamp["sec"].as_i64().unwrap_or(0),
                    g.stamp["nanosec"].as_u64().unwrap_or(0),
                ) <= stamp;
                if g.result.is_none()
                    && (exact || (all && stamp == (0, 0)) || (stamp != (0, 0) && before))
                {
                    g.status = 3;
                    cancelling.push(json!({"goal_id":g.uuid,"stamp":g.stamp}));
                    notify.push(g.id.clone());
                }
            }
            let code = if !cancelling.is_empty() || all {
                0
            } else if terminal {
                3
            } else if !known {
                2
            } else {
                1
            };
            self.backend.respond(
                entity,
                request,
                &json!({"return_code":code,"goals_canceling":cancelling}),
            )?;
            for id in notify {
                self.send(
                    owner,
                    json!({"op":"cancel_action_goal","action":action}),
                    &Some(id),
                    &Options::default(),
                    None,
                );
            }
            self.publish_status(&action)?;
        }
        Ok(true)
    }
}
