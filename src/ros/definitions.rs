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

use super::message::MessageType;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::PathBuf,
};

pub(super) fn interface_path(typ: &str) -> Result<PathBuf> {
    let parts: Vec<_> = typ.split('/').collect();
    anyhow::ensure!(
        parts.len() == 3
            && parts.iter().all(|part| !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')),
        "invalid interface name"
    );
    let prefixes = env::var_os("AMENT_PREFIX_PATH").unwrap_or_default();
    env::split_paths(&prefixes)
        .map(|prefix| {
            prefix.join(format!(
                "share/{}/{}/{}.{}",
                parts[0], parts[1], parts[2], parts[1]
            ))
        })
        .find(|path| path.is_file())
        .with_context(|| format!("interface source not found: {typ}"))
}
pub(super) fn interfaces() -> Vec<String> {
    let prefixes = env::var_os("AMENT_PREFIX_PATH").unwrap_or_default();
    let mut result = std::collections::BTreeSet::new();
    for prefix in env::split_paths(&prefixes) {
        let Ok(entries) =
            fs::read_dir(prefix.join("share/ament_index/resource_index/rosidl_interfaces"))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let package = entry.file_name().to_string_lossy().into_owned();
            if let Ok(text) = fs::read_to_string(entry.path()) {
                for line in text.lines() {
                    if let Some((name, _)) = line.rsplit_once('.') {
                        result.insert(format!("{package}/{name}"));
                    }
                }
            }
        }
    }
    result.into_iter().collect()
}
pub(super) fn raw(typ: &str) -> Result<String> {
    let typ = crate::backend::type_name(typ, "msg")?;
    let mut stack = vec![typ.clone()];
    let mut seen = HashSet::new();
    let mut output = String::new();
    let mut first = true;
    while let Some(current) = stack.pop() {
        let text = fs::read_to_string(interface_path(&current)?)?;
        if !first {
            output.push_str(&format!("\n================================================================================\nMSG: {}\n", current.replace("/msg/","/")));
        }
        first = false;
        output.push_str(&text);
        // C metadata resolves aliases, arrays and bounded sequences without parsing IDL.
        for dependency in MessageType::load(&current)?.dependencies() {
            if seen.insert(dependency.clone()) {
                stack.push(dependency);
            }
        }
    }
    Ok(output)
}
pub(super) fn constants(typ: &str) -> BTreeMap<String, String> {
    let mut parts: Vec<_> = typ.split('/').collect();
    if parts.len() != 3 {
        return BTreeMap::new();
    }
    let (base, section) = if parts[1] == "srv" {
        if let Some(base) = parts[2].strip_suffix("_Request") {
            (base, 0)
        } else if let Some(base) = parts[2].strip_suffix("_Response") {
            (base, 1)
        } else {
            (parts[2], 0)
        }
    } else if parts[1] == "action" {
        if let Some(base) = parts[2].strip_suffix("_Goal") {
            (base, 0)
        } else if let Some(base) = parts[2].strip_suffix("_Result") {
            (base, 1)
        } else if let Some(base) = parts[2].strip_suffix("_Feedback") {
            (base, 2)
        } else {
            (parts[2], 0)
        }
    } else {
        (parts[2], 0)
    };
    parts[2] = base;
    let Ok(path) = interface_path(&parts.join("/")) else {
        return BTreeMap::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let section = text.split("\n---").nth(section).unwrap_or("");
    let mut result = BTreeMap::new();
    for line in section.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((field, value)) = line.split_once('=') else {
            continue;
        };
        let words: Vec<_> = field.split_whitespace().collect();
        if words.len() != 2 {
            continue;
        }
        let value = if words[0] == "string" {
            value.trim().trim_matches(['\'', '"']).to_string()
        } else {
            value.split('#').next().unwrap_or("").trim().to_string()
        };
        result.insert(words[1].to_string(), value);
    }
    result
}
#[derive(Default)]
pub(super) struct Definitions {
    raw: BTreeMap<String, String>,
    details: BTreeMap<String, Value>,
}
impl Definitions {
    pub(super) fn raw(&mut self, typ: &str) -> String {
        self.raw
            .entry(typ.into())
            .or_insert_with(|| {
                raw(typ).unwrap_or_else(|e| {
                    format!("# failed to get full definition text for {typ}: {e}")
                })
            })
            .clone()
    }
    pub(super) fn details(&mut self, typ: &str) -> Value {
        self.details
            .entry(typ.into())
            .or_insert_with(|| {
                MessageType::load(typ)
                    .and_then(|t| t.typedefs())
                    .unwrap_or_else(|_| json!([]))
            })
            .clone()
    }
}
