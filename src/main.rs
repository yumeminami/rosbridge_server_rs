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

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser};
mod config;
mod logging;
use std::net::SocketAddr;

#[cfg(feature = "ros2")]
mod server;

#[derive(Parser, Debug)]
#[command(version, about = "ROS 2 rosbridge WebSocket server")]
struct Args {
    /// Read settings from a TOML file. Explicit command-line flags take precedence.
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    /// Write rotating logs to this directory.
    #[arg(long)]
    log_directory: Option<std::path::PathBuf>,
    /// Log filter, for example info or rosbridge_server_rs=debug.
    #[arg(long)]
    log_level: Option<String>,
    /// Timestamp timezone (default: local).
    #[arg(long, value_enum)]
    log_timezone: Option<config::Timezone>,
    /// Enable or disable terminal colors; field names stay plain (default: true).
    #[arg(long, action = clap::ArgAction::Set)]
    log_ansi: Option<bool>,
    #[arg(long, default_value = "/")]
    url_path: String,
    #[arg(long, default_value_t = 256)]
    incoming_queue_size: usize,
    #[arg(long, default_value_t = 64)]
    write_queue_size: usize,
    #[arg(long, default_value_t = 30.0)]
    fragment_timeout: f64,
    #[arg(long, default_value = "0.0.0.0:9090")]
    bind: SocketAddr,
    #[arg(long, default_value = "rosbridge_websocket")]
    node_name: String,
    #[arg(long, default_value = "/")]
    namespace: String,
    #[arg(long)]
    use_sim_time: bool,
    /// Disable built-in rosapi services when another node provides them.
    #[arg(long)]
    no_rosapi: bool,
    #[arg(long, default_value_t = 30.0)]
    service_timeout: f64,
    #[arg(long, default_value_t = 16777216)]
    max_message_size: usize,
    /// Arguments after -- are forwarded to RCL (e.g. -- --ros-args -r __ns:=/robot).
    #[arg(last = true)]
    ros_args: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let matches = Args::command().get_matches();
    let mut args = Args::from_arg_matches(&matches)?;
    let log = config::load(&mut args, &matches)?;
    let _log_guard = logging::init(&log)?;
    #[cfg(feature = "ros2")]
    {
        server::run(args).await
    }
    #[cfg(not(feature = "ros2"))]
    {
        let _ = args;
        anyhow::bail!(
            "ROS support is disabled; source ROS 2 and build with the default ros2 feature"
        )
    }
}
