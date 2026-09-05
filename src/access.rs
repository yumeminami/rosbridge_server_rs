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

//! Shared allowlists for WebSocket forwarding and native rosapi discovery.

use anyhow::{Context, Result, ensure};
use glob::Pattern;

#[derive(Default)]
pub struct Access {
    pub(crate) pub_topics: Option<Vec<Pattern>>,
    pub(crate) sub_topics: Option<Vec<Pattern>>,
    pub(crate) services: Option<Vec<Pattern>>,
    pub(crate) params: Option<Vec<Pattern>>,
}

pub(crate) fn matches(patterns: &Option<Vec<Pattern>>, name: &str) -> bool {
    patterns
        .as_ref()
        .is_none_or(|patterns| patterns.iter().any(|p| p.matches(name)))
}

fn raw_parameter_service(typ: &str) -> bool {
    matches!(
        typ,
        "rcl_interfaces/srv/GetParameters"
            | "rcl_interfaces/srv/GetParameterTypes"
            | "rcl_interfaces/srv/SetParameters"
            | "rcl_interfaces/srv/SetParametersAtomically"
            | "rcl_interfaces/srv/ListParameters"
            | "rcl_interfaces/srv/DescribeParameters"
    )
}

fn rosapi_parameter_service(typ: &str) -> bool {
    matches!(
        typ,
        "rosapi_msgs/srv/GetParam"
            | "rosapi_msgs/srv/SetParam"
            | "rosapi_msgs/srv/HasParam"
            | "rosapi_msgs/srv/DeleteParam"
            | "rosapi_msgs/srv/GetParamNames"
    )
}

impl Access {
    pub fn from_ros_args(args: &[String]) -> Result<Self> {
        let parse = |name: &str| -> Result<Option<Vec<Pattern>>> {
            let value = args
                .windows(2)
                .filter(|pair| pair[0] == "-p" || pair[0] == "--param")
                .filter_map(|pair| pair[1].split_once(":="))
                .filter(|(key, _)| key.rsplit(':').next() == Some(name))
                .map(|(_, value)| value)
                .next_back();
            value
                .map(|value| {
                    let patterns =
                        match toml::from_str::<toml::Table>(&format!("patterns = {value}")) {
                            Ok(table) => table["patterns"]
                                .as_array()
                                .context("expected an array of glob strings")?
                                .iter()
                                .map(|v| {
                                    v.as_str()
                                        .map(str::to_owned)
                                        .context("expected a glob string")
                                })
                                .collect::<Result<Vec<_>>>()?,
                            Err(_) => {
                                // ROS CLI also accepts unquoted YAML flow-list strings.
                                let inner = value
                                    .trim()
                                    .strip_prefix('[')
                                    .and_then(|v| v.strip_suffix(']'))
                                    .with_context(|| {
                                        format!("{name} must be an array of glob strings")
                                    })?;
                                ensure!(
                                    !inner.contains(['\'', '"']),
                                    "invalid quoted glob array for {name}"
                                );
                                if inner.trim().is_empty() {
                                    Vec::new()
                                } else {
                                    inner.split(',').map(|p| p.trim().to_owned()).collect()
                                }
                            }
                        };
                    patterns.iter().map(|p| Ok(Pattern::new(p)?)).collect()
                })
                .transpose()
        };
        let legacy = parse("topics_glob")?;
        let merge = |specific: Option<Vec<Pattern>>| match specific {
            None => legacy.clone(),
            Some(mut patterns) => {
                patterns.extend(legacy.clone().unwrap_or_default());
                Some(patterns)
            }
        };
        Ok(Self {
            pub_topics: merge(parse("topics_pub_glob")?),
            sub_topics: merge(parse("topics_sub_glob")?),
            services: parse("services_glob")?,
            params: parse("params_glob")?,
        })
    }

    pub(crate) fn topic(&self, name: &str, publish: bool) -> Result<()> {
        let (patterns, setting) = if publish {
            (&self.pub_topics, "topics_pub_glob")
        } else {
            (&self.sub_topics, "topics_sub_glob")
        };
        ensure!(matches(patterns, name), "{setting} denies {name}");
        Ok(())
    }

    pub(crate) fn service(&self, name: &str) -> Result<()> {
        ensure!(matches(&self.services, name), "services_glob denies {name}");
        Ok(())
    }

    pub(crate) fn parameter_service(&self, typ: &str, args: &serde_json::Value) -> Result<()> {
        if self.params.is_none() {
            return Ok(());
        }
        ensure!(
            !raw_parameter_service(typ),
            "params_glob restricts raw parameter services; use /rosapi parameter services"
        );
        if rosapi_parameter_service(typ) && typ != "rosapi_msgs/srv/GetParamNames" {
            let full = args["name"]
                .as_str()
                .context("parameter name must be a string")?;
            let (_, name) = full
                .split_once(':')
                .context("expected <node_name>:<param_name>")?;
            ensure!(matches(&self.params, name), "params_glob denies {name}");
        }
        Ok(())
    }

    pub(crate) fn advertise_service(&self, typ: &str) -> Result<()> {
        ensure!(
            self.params.is_none()
                || (!raw_parameter_service(typ) && !rosapi_parameter_service(typ)),
            "params_glob forbids client-advertised parameter services"
        );
        Ok(())
    }

    pub(crate) fn action(&self, name: &str, advertise: bool) -> Result<()> {
        for suffix in ["send_goal", "get_result", "cancel_goal"] {
            self.service(&format!("{name}/_action/{suffix}"))?;
        }
        for suffix in ["feedback", "status"] {
            self.topic(&format!("{name}/_action/{suffix}"), advertise)?;
        }
        Ok(())
    }
}
