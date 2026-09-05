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

//! ROS backend contract and QoS values shared by the protocol and native implementation.

use anyhow::{Result, bail, ensure};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Entity = u64;
#[derive(Clone, Debug)]
pub struct RosMessage {
    pub json: Value,
    pub cbor: ciborium::Value,
    pub raw: Vec<u8>,
    pub stamp: (i64, u32),
}

#[derive(Debug)]
pub enum Event {
    Message(Entity, RosMessage),
    Request {
        entity: Entity,
        request: u64,
        args: Value,
    },
    Response {
        entity: Entity,
        sequence: i64,
        values: Value,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Qos {
    pub history: u32,
    pub depth: usize,
    pub reliability: u32,
    pub durability: u32,
    pub deadline: (u64, u64),
    pub lifespan: (u64, u64),
}

impl Qos {
    pub fn publisher() -> Self {
        Self {
            history: 1,
            depth: 100,
            reliability: 1,
            durability: 1,
            ..Self::default()
        }
    }

    pub fn subscriber() -> Self {
        Self {
            history: 1,
            depth: 10,
            reliability: 2,
            durability: 2,
            ..Self::default()
        }
    }

    pub fn service() -> Self {
        Self {
            history: 1,
            depth: 10,
            reliability: 1,
            durability: 2,
            ..Self::default()
        }
    }

    pub fn parse(value: &Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("qos must be an object"))?;
        let mut qos = Self::default();
        for (key, value) in object {
            match key.as_str() {
                "history" => qos.history = policy(value, &[("keep_last", 1), ("keep_all", 2)])?,
                "reliability" => {
                    qos.reliability = policy(
                        value,
                        &[("reliable", 1), ("best_effort", 2), ("best_available", 4)],
                    )?
                }
                "durability" => {
                    qos.durability = policy(
                        value,
                        &[
                            ("transient_local", 1),
                            ("volatile", 2),
                            ("best_available", 4),
                        ],
                    )?
                }
                "depth" => {
                    qos.depth = value
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("depth must be a nonnegative integer"))?
                        .try_into()?
                }
                "deadline"
                    if value
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("best_available")) =>
                {
                    qos.deadline = (9223372036, 854775806)
                }
                "deadline" => qos.deadline = duration(value)?,
                "lifespan" => qos.lifespan = duration(value)?,
                _ => bail!("unknown QoS field: {key}"),
            }
        }
        Ok(qos)
    }
}

fn policy(value: &Value, names: &[(&str, u32)]) -> Result<u32> {
    let name = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("QoS policy must be a string"))?;
    names
        .iter()
        .find(|(s, _)| s.eq_ignore_ascii_case(name))
        .map(|(_, v)| *v)
        .ok_or_else(|| anyhow::anyhow!("invalid QoS policy: {name}"))
}

fn duration(v: &Value) -> Result<(u64, u64)> {
    if v.as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("infinite"))
    {
        return Ok((9223372036, 854775807));
    }
    if let Some(seconds) = v.as_f64() {
        ensure!(
            seconds.is_finite() && seconds >= 0.0 && seconds < u64::MAX as f64,
            "invalid duration"
        );
        let d = std::time::Duration::try_from_secs_f64(seconds)?;
        return Ok((d.as_secs(), d.subsec_nanos() as u64));
    }
    let secs = v["secs"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("duration requires nonnegative secs and nsecs"))?;
    let nanos = v["nsecs"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("duration requires nonnegative secs and nsecs"))?;
    Ok((
        secs.checked_add(nanos / 1_000_000_000)
            .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
        nanos % 1_000_000_000,
    ))
}

pub fn type_name(name: &str, category: &str) -> Result<String> {
    let parts: Vec<_> = name.split('/').collect();
    let mut parts = match parts.as_slice() {
        [package, typ] => vec![*package, category, *typ],
        [package, kind, typ] if *kind == category => vec![*package, *kind, *typ],
        _ => bail!("invalid {category} type: {name}"),
    };
    ensure!(
        parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')),
        "invalid interface type"
    );
    // ROS 1 clients still use the rosapi package name for ROS 2 services.
    if category == "srv" && parts[0] == "rosapi" {
        parts[0] = "rosapi_msgs";
    }
    Ok(parts.join("/"))
}

/// All ROS entities are owned and accessed by the bridge's single worker thread.
/// The WebSocket tasks only send commands; no ROS pointer crosses threads.
pub trait Backend {
    fn publisher(&mut self, name: &str, typ: &str, qos: Qos) -> Result<Entity>;
    fn subscription(&mut self, name: &str, typ: &str, qos: Qos) -> Result<Entity>;
    fn client(&mut self, name: &str, typ: &str) -> Result<Entity>;
    fn service(&mut self, name: &str, typ: &str) -> Result<Entity>;
    fn publish(&mut self, entity: Entity, message: &Value) -> Result<()>;
    fn request(&mut self, entity: Entity, args: &Value) -> Result<i64>;
    /// Validate a service response without sending it or consuming a request.
    fn validate_response(&self, entity: Entity, values: &Value) -> Result<()>;
    fn respond(&mut self, entity: Entity, request: u64, values: &Value) -> Result<()>;
    fn discard_request(&mut self, entity: Entity, request: u64);
    fn destroy(&mut self, entity: Entity);
    fn poll(&mut self) -> Result<Vec<Event>>;
    fn topics(&mut self) -> Result<BTreeMap<String, Vec<String>>>;
    fn services(&mut self) -> Result<BTreeMap<String, Vec<String>>>;
    fn publisher_qos(&mut self, topic: &str) -> Result<Vec<Qos>>;
    fn now(&self) -> (i64, u32);
}
