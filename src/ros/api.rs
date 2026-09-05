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

#[path = "parameters.rs"]
mod parameters;

use super::{
    Ros,
    definitions::{self, Definitions},
    graph,
};
use crate::backend::{Backend, Entity, Event, type_name};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const SERVICES: &[(&str, &str)] = &[
    ("topics", "Topics"),
    ("interfaces", "Interfaces"),
    ("topics_for_type", "TopicsForType"),
    ("topics_and_raw_types", "TopicsAndRawTypes"),
    ("services", "Services"),
    ("services_for_type", "ServicesForType"),
    ("nodes", "Nodes"),
    ("node_details", "NodeDetails"),
    ("action_servers", "GetActionServers"),
    ("action_type", "ActionType"),
    ("topic_type", "TopicType"),
    ("service_type", "ServiceType"),
    ("publishers", "Publishers"),
    ("subscribers", "Subscribers"),
    ("service_providers", "ServiceProviders"),
    ("service_node", "ServiceNode"),
    ("message_details", "MessageDetails"),
    ("service_request_details", "ServiceRequestDetails"),
    ("service_response_details", "ServiceResponseDetails"),
    ("action_goal_details", "ActionGoalDetails"),
    ("action_result_details", "ActionResultDetails"),
    ("action_feedback_details", "ActionFeedbackDetails"),
    ("set_param", "SetParam"),
    ("get_param", "GetParam"),
    ("has_param", "HasParam"),
    ("delete_param", "DeleteParam"),
    ("get_param_names", "GetParamNames"),
    ("get_time", "GetTime"),
    ("get_ros_version", "GetROSVersion"),
];

#[derive(Default)]
pub(super) struct Api {
    operations: HashMap<Entity, &'static str>,
    definitions: Definitions,
    pending: HashMap<(Entity, i64), Pending>,
    groups: HashMap<u64, Group>,
    config: Config,
    parameter_timeout: Duration,
}
struct Pending {
    group: u64,
    node: String,
    delete_check: bool,
}
struct Group {
    service: Entity,
    request: u64,
    op: &'static str,
    args: Value,
    remaining: usize,
    names: Vec<String>,
    deadline: Instant,
}
#[derive(Default)]
struct Config {
    pub_topics: Option<Vec<glob::Pattern>>,
    sub_topics: Option<Vec<glob::Pattern>>,
    services: Option<Vec<glob::Pattern>>,
    params: Option<Vec<glob::Pattern>>,
}
fn matches(patterns: &Option<Vec<glob::Pattern>>, name: &str) -> bool {
    patterns
        .as_ref()
        .is_none_or(|patterns| patterns.iter().any(|p| p.matches(name)))
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
impl Api {
    pub(super) fn new(ros: &mut Ros, args: &[String]) -> Result<Self> {
        let mut api = Self::default();
        let mut values = HashMap::new();
        for pair in args.windows(2) {
            if (pair[0] == "-p" || pair[0] == "--param")
                && let Some((key, value)) = pair[1].split_once(":=")
            {
                values.insert(
                    key.rsplit(':').next().unwrap().to_string(),
                    value.to_string(),
                );
            }
        }
        let parse = |name: &str| -> Result<Option<Vec<glob::Pattern>>> {
            let Some(value) = values.get(name).filter(|v| !v.is_empty()) else {
                return Ok(None);
            };
            let value = value.trim_matches(['[', ']']);
            Ok(Some(
                value
                    .split(',')
                    .map(|s| s.trim().trim_matches(['\'', '"']))
                    .filter(|s| !s.is_empty())
                    .map(glob::Pattern::new)
                    .collect::<std::result::Result<_, _>>()?,
            ))
        };
        api.parameter_timeout = Duration::try_from_secs_f64(
            values
                .get("params_timeout")
                .map(|v| v.parse::<f64>())
                .transpose()?
                .unwrap_or(5.0),
        )?;
        anyhow::ensure!(
            !api.parameter_timeout.is_zero(),
            "params_timeout must be positive"
        );
        let legacy = parse("topics_glob")?;
        let merge = |specific: Option<Vec<glob::Pattern>>| match specific {
            None => legacy.clone(),
            Some(mut p) => {
                p.extend(legacy.clone().unwrap_or_default());
                Some(p)
            }
        };
        api.config = Config {
            pub_topics: merge(parse("topics_pub_glob")?),
            sub_topics: merge(parse("topics_sub_glob")?),
            services: parse("services_glob")?,
            params: parse("params_glob")?,
        };
        for &(name, typ) in SERVICES {
            let entity = ros.service(
                &format!("/rosapi/{name}"),
                &format!("rosapi_msgs/srv/{typ}"),
            )?;
            api.operations.insert(entity, name);
        }
        Ok(api)
    }
    fn topic_visible(&self, name: &str) -> bool {
        graph::public(name)
            && (matches(&self.config.pub_topics, name) || matches(&self.config.sub_topics, name))
    }
    fn reply(
        &mut self,
        ros: &mut Ros,
        entity: Entity,
        request: u64,
        op: &'static str,
        args: &Value,
    ) -> Result<()> {
        if [
            "get_param",
            "set_param",
            "has_param",
            "delete_param",
            "get_param_names",
        ]
        .contains(&op)
        {
            return self.parameter(ros, entity, request, op, args.clone());
        }
        let value = match op {
            "topics" | "topics_and_raw_types" | "topics_for_type" | "topic_type" => {
                let graph = ros.topics()?;
                let pairs: Vec<_> = graph
                    .iter()
                    .filter(|(name, _)| self.topic_visible(name))
                    .filter_map(|(name, types)| {
                        types.first().map(|typ| (name.clone(), typ.clone()))
                    })
                    .collect();
                match op {
                    "topics_for_type" => {
                        json!({"topics":pairs.iter().filter(|(_,typ)|typ==text(args,"type")).map(|(name,_)|name).collect::<Vec<_>>()})
                    }
                    "topic_type" => {
                        json!({"type":pairs.iter().find(|(name,_)|name==text(args,"topic")).map(|(_,typ)|typ.as_str()).unwrap_or("")})
                    }
                    _ => {
                        let mut v = json!({"topics":pairs.iter().map(|(n,_)|n).collect::<Vec<_>>(),"types":pairs.iter().map(|(_,t)|t).collect::<Vec<_>>()});
                        if op == "topics_and_raw_types" {
                            v["typedefs_full_text"] = json!(
                                pairs
                                    .iter()
                                    .map(|(_, typ)| self.definitions.raw(typ))
                                    .collect::<Vec<_>>()
                            );
                        }
                        v
                    }
                }
            }
            "interfaces" => json!({"interfaces":definitions::interfaces()}),
            "services" | "services_for_type" | "service_type" => {
                let graph = ros.services()?;
                let visible =
                    |name: &str| graph::public(name) && matches(&self.config.services, name);
                if op == "service_type" {
                    let name = text(args, "service");
                    json!({"type":if visible(name){graph.get(name).and_then(|t|t.first()).map(String::as_str).unwrap_or("")}else{""}})
                } else {
                    json!({"services":graph.iter().filter(|(name,types)|visible(name)&&(op=="services"||types.first().is_some_and(|typ|typ==text(args,"type")))).map(|(name,_)|name).collect::<Vec<_>>()})
                }
            }
            "nodes" => json!({"nodes":ros.nodes()?}),
            "node_details" => {
                let name = text(args, "node");
                json!({"subscribing":ros.node_graph(name,"subscribers")?.keys().collect::<Vec<_>>(),"publishing":ros.node_graph(name,"publishers")?.keys().collect::<Vec<_>>(),"services":ros.node_graph(name,"services")?.keys().collect::<Vec<_>>()})
            }
            "publishers" | "subscribers" | "service_node" | "service_providers" => {
                let query = text(
                    args,
                    if op == "publishers" || op == "subscribers" {
                        "topic"
                    } else {
                        "service"
                    },
                );
                let patterns = match op {
                    "publishers" => &self.config.pub_topics,
                    "subscribers" => &self.config.sub_topics,
                    _ => &self.config.services,
                };
                let mut names = Vec::new();
                if matches(patterns, query) {
                    for node in ros.nodes()? {
                        // Nodes may disappear during graph enumeration.
                        let Ok(graph) = ros.node_graph(&node, op) else {
                            continue;
                        };
                        if if op == "service_providers" {
                            graph
                                .values()
                                .any(|types| types.iter().any(|typ| typ == query))
                        } else {
                            graph.contains_key(query)
                        } {
                            names.push(node);
                        }
                    }
                }
                match op {
                    "service_node" => {
                        json!({"node":names.first().map(String::as_str).unwrap_or("")})
                    }
                    "service_providers" => json!({"providers":names}),
                    _ => json!({op:names}),
                }
            }
            "action_servers" => {
                let topics = ros.topics()?;
                json!({"action_servers":topics.keys().filter_map(|n|n.strip_suffix("/_action/feedback")).filter(|name|topics.contains_key(&format!("{name}/_action/status")) && (matches(&self.config.pub_topics,name)||matches(&self.config.sub_topics,name))).collect::<Vec<_>>()})
            }
            "action_type" => {
                let graph = ros.services()?;
                let name = format!("{}/_action/send_goal", text(args, "action"));
                json!({"type":graph.get(&name).and_then(|t|t.first()).and_then(|t|t.strip_suffix("_SendGoal")).unwrap_or("")})
            }
            "message_details"
            | "service_request_details"
            | "service_response_details"
            | "action_goal_details"
            | "action_result_details"
            | "action_feedback_details" => {
                let (kind, suffix) = match op {
                    "message_details" => ("msg", ""),
                    "service_request_details" => ("srv", "_Request"),
                    "service_response_details" => ("srv", "_Response"),
                    "action_goal_details" => ("action", "_Goal"),
                    "action_result_details" => ("action", "_Result"),
                    _ => ("action", "_Feedback"),
                };
                let details = type_name(text(args, "type"), kind)
                    .map(|typ| self.definitions.details(&format!("{typ}{suffix}")))
                    .unwrap_or_else(|_| json!([]));
                json!({"typedefs":details})
            }
            "get_time" => {
                let (sec, nanosec) = ros.now();
                json!({"time":{"sec":sec,"nanosec":nanosec}})
            }
            "get_ros_version" => {
                json!({"version":2,"distro":std::env::var("ROS_DISTRO").unwrap_or_default()})
            }
            _ => bail!("unknown rosapi operation {op}"),
        };
        ros.respond(entity, request, &value)
    }
    pub(super) fn process(&mut self, ros: &mut Ros, events: Vec<Event>) -> Result<Vec<Event>> {
        let mut output = Vec::new();
        for event in events {
            match event {
                Event::Request {
                    entity,
                    request,
                    args,
                } if self.operations.contains_key(&entity) => {
                    let op = self.operations[&entity];
                    if let Err(error) = self.reply(ros, entity, request, op, &args) {
                        tracing::warn!(%op,"rosapi request: {error:#}");
                        ros.discard_request(entity, request);
                    }
                }
                Event::Response {
                    entity,
                    sequence,
                    values,
                } if self.pending.contains_key(&(entity, sequence)) => {
                    let pending = self.pending.remove(&(entity, sequence)).unwrap();
                    ros.destroy(entity);
                    self.parameter_response(ros, pending, &values)?;
                }
                event => output.push(event),
            }
        }
        let expired: Vec<_> = self
            .groups
            .iter()
            .filter(|(_, g)| g.deadline <= Instant::now())
            .map(|(&id, _)| id)
            .collect();
        for id in expired {
            self.finish(ros, id, None, Some("Timeout occurred".into()))?;
        }
        Ok(output)
    }
}
