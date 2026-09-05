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
use clap::Parser;
use std::net::SocketAddr;

#[cfg(feature = "ros2")]
mod server;

#[derive(Parser, Debug)]
#[command(version, about = "ROS 2 rosbridge WebSocket server")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:9090")]
    bind: SocketAddr,
    #[arg(long, default_value = "rosbridge_websocket")]
    node_name: String,
    #[arg(long, default_value = "/")]
    namespace: String,
    #[arg(long)]
    use_sim_time: bool,
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
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
