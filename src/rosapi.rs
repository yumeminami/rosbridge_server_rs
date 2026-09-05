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

use anyhow::{Context, Result};
use std::env;
use tokio::process::{Child, Command};

pub(super) fn start(use_sim_time: bool) -> Result<Child> {
    let prefixes = env::var_os("AMENT_PREFIX_PATH").unwrap_or_default();
    let executable = env::split_paths(&prefixes)
        .map(|prefix| prefix.join("lib/rosapi/rosapi_node"))
        .find(|path| path.is_file())
        .context("rosapi_node not found; install ros-jazzy-rosapi and source its ROS environment, or use --no-rosapi for an independently managed node")?;
    let child = Command::new(executable)
        .args(["--ros-args", "-r", "__node:=rosapi", "-r", "__ns:=/", "-p"])
        .arg(format!("use_sim_time:={use_sim_time}"))
        .kill_on_drop(true)
        .spawn()
        .context("start rosapi_node")?;
    tracing::info!(pid = child.id(), "started rosapi node");
    Ok(child)
}
