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

use super::*;

impl Api {
    pub(super) fn finish(
        &mut self,
        ros: &mut Ros,
        id: u64,
        value: Option<Value>,
        error: Option<String>,
    ) -> Result<()> {
        let group = self
            .groups
            .remove(&id)
            .context("missing parameter request")?;
        let clients: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.group == id)
            .map(|(&(e, s), _)| (e, s))
            .collect();
        for key in clients {
            self.pending.remove(&key);
            ros.destroy(key.0);
        }
        let response = match group.op {
            "get_param_names" => json!({"names":group.names}),
            "has_param" => {
                json!({"exists":error.is_none()&&value.as_ref().is_some_and(|v|v["type"].as_u64().is_some_and(|t|(1..=9).contains(&t)))})
            }
            "get_param" => {
                let default = serde_json::from_str::<Value>(text(&group.args, "default_value"))
                    .unwrap_or(json!(""));
                let result = if error.is_none() {
                    value.as_ref().map(parameter_json).unwrap_or(default)
                } else {
                    default
                };
                #[cfg(ros_humble)]
                {
                    json!({"value":result.to_string()})
                }
                #[cfg(not(ros_humble))]
                {
                    json!({"value":result.to_string(),"successful":error.is_none(),"reason":error.unwrap_or_default()})
                }
            }
            _ => {
                #[cfg(ros_humble)]
                {
                    json!({})
                }
                #[cfg(not(ros_humble))]
                {
                    json!({"successful":error.is_none(),"reason":error.unwrap_or_default()})
                }
            }
        };
        ros.respond(group.service, group.request, &response)
    }
    pub(super) fn parameter(
        &mut self,
        ros: &mut Ros,
        service: Entity,
        request: u64,
        op: &'static str,
        args: Value,
    ) -> Result<()> {
        self.groups.insert(
            request,
            Group {
                service,
                request,
                op,
                args: args.clone(),
                remaining: 0,
                names: Vec::new(),
                deadline: Instant::now() + self.parameter_timeout,
            },
        );
        let result = (|| -> Result<()> {
            let graph = ros.services()?;
            let mut calls = Vec::new();
            if op == "get_param_names" {
                for node in ros.nodes()? {
                    let service = format!("{node}/list_parameters");
                    if graph.contains_key(&service) {
                        calls.push((
                            node,
                            service,
                            "ListParameters",
                            json!({"prefixes":[],"depth":0}),
                        ));
                    }
                }
            } else {
                let (node, name) = text(&args, "name")
                    .split_once(':')
                    .context("expected <node_name>:<param_name>")?;
                anyhow::ensure!(
                    matches(&self.config.params, name),
                    "Parameter {name} does not match any of the glob strings"
                );
                let node = format!("/{}", node.trim_start_matches('/'));
                let (method, typ, payload) =
                    if op == "get_param" || op == "has_param" || op == "delete_param" {
                        ("get_parameters", "GetParameters", json!({"names":[name]}))
                    } else {
                        let value = if op == "delete_param" {
                            Value::Null
                        } else {
                            serde_json::from_str(text(&args, "value"))?
                        };
                        (
                            "set_parameters",
                            "SetParameters",
                            json!({"parameters":[{"name":name,"value":parameter_value(&value)?}]}),
                        )
                    };
                let service = format!("{node}/{method}");
                anyhow::ensure!(
                    graph.contains_key(&service),
                    "Service {service} is not available"
                );
                calls.push((node, service, typ, payload));
            }
            for (node, service, typ, payload) in calls {
                let entity = ros.client(&service, &format!("rcl_interfaces/srv/{typ}"))?;
                let sequence = match ros.request(entity, &payload) {
                    Ok(s) => s,
                    Err(e) => {
                        ros.destroy(entity);
                        return Err(e);
                    }
                };
                self.pending.insert(
                    (entity, sequence),
                    Pending {
                        group: request,
                        node,
                        delete_check: op == "delete_param",
                    },
                );
                self.groups.get_mut(&request).unwrap().remaining += 1;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.finish(ros, request, None, Some(error.to_string()))?;
        } else if self.groups[&request].remaining == 0 {
            self.finish(ros, request, None, None)?;
        }
        Ok(())
    }
    pub(super) fn parameter_response(
        &mut self,
        ros: &mut Ros,
        pending: Pending,
        response: &Value,
    ) -> Result<()> {
        let group = self.groups.get_mut(&pending.group).unwrap();
        group.remaining -= 1;
        if pending.delete_check {
            let exists = response["values"]
                .as_array()
                .and_then(|v| v.first())
                .is_some_and(|v| v["type"].as_u64().is_some_and(|typ| typ > 0));
            if !exists {
                return self.finish(ros, pending.group, None, None);
            }
            let name = text(&group.args, "name")
                .split_once(':')
                .unwrap()
                .1
                .to_string();
            let entity = ros.client(
                &format!("{}/set_parameters", pending.node),
                "rcl_interfaces/srv/SetParameters",
            )?;
            let sequence = match ros.request(
                entity,
                &json!({"parameters":[{"name":name,"value":{"type":0}}]}),
            ) {
                Ok(sequence) => sequence,
                Err(error) => {
                    ros.destroy(entity);
                    return self.finish(ros, pending.group, None, Some(error.to_string()));
                }
            };
            group.remaining += 1;
            self.pending.insert(
                (entity, sequence),
                Pending {
                    delete_check: false,
                    ..pending
                },
            );
            return Ok(());
        }
        match group.op {
            "get_param_names" => {
                if let Some(names) = response["result"]["names"].as_array() {
                    for name in names.iter().filter_map(Value::as_str) {
                        let full = format!("{}:{name}", pending.node);
                        if matches(&self.config.params, name) {
                            group.names.push(full);
                        }
                    }
                }
                if group.remaining == 0 {
                    self.finish(ros, pending.group, None, None)?;
                }
            }
            "get_param" | "has_param" => {
                let value = response["values"]
                    .as_array()
                    .and_then(|v| v.first())
                    .cloned();
                let error = if value.as_ref().is_none_or(|v| v["type"] == 0) {
                    Some("Parameter not found".into())
                } else {
                    None
                };
                self.finish(ros, pending.group, value, error)?;
            }
            _ => {
                let result = &response["results"][0];
                let error = if result["successful"] == true {
                    None
                } else {
                    Some(text(result, "reason").to_string())
                };
                self.finish(ros, pending.group, None, error)?;
            }
        }
        Ok(())
    }
}

fn parameter_json(value: &Value) -> Value {
    let field = match value["type"].as_u64().unwrap_or(0) {
        1 => "bool_value",
        2 => "integer_value",
        3 => "double_value",
        4 => "string_value",
        5 => "byte_array_value",
        6 => "bool_array_value",
        7 => "integer_array_value",
        8 => "double_array_value",
        9 => "string_array_value",
        _ => return Value::Null,
    };
    value[field].clone()
}
fn parameter_value(value: &Value) -> Result<Value> {
    let (typ, field) = match value {
        Value::Null => return Ok(json!({"type":0})),
        Value::Bool(_) => (1, "bool_value"),
        Value::Number(n) if n.is_i64() => (2, "integer_value"),
        Value::Number(_) => (3, "double_value"),
        Value::String(_) => (4, "string_value"),
        Value::Array(v) if v.iter().all(Value::is_boolean) => (6, "bool_array_value"),
        Value::Array(v) if v.iter().all(Value::is_i64) => (7, "integer_array_value"),
        Value::Array(v) if v.iter().all(Value::is_number) => (8, "double_array_value"),
        Value::Array(v) if v.iter().all(Value::is_string) => (9, "string_array_value"),
        _ => bail!("parameter value must be a scalar or homogeneous array"),
    };
    Ok(json!({"type":typ,field:value}))
}
