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

//! Console output and optional rotating file logs.
use crate::config::Log;
use anyhow::{Result, bail, ensure};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{EnvFilter, prelude::*};

pub fn init(config: &Log) -> Result<Option<WorkerGuard>> {
    let filter =
        EnvFilter::try_new(std::env::var("RUST_LOG").unwrap_or_else(|_| config.level.clone()))?;
    ensure!(
        config.console || config.directory.is_some(),
        "enable console logging or set log.directory"
    );
    let rotation = match config.rotation.as_str() {
        "daily" => Rotation::DAILY,
        "hourly" => Rotation::HOURLY,
        "never" => Rotation::NEVER,
        _ => bail!("log.rotation must be daily, hourly or never"),
    };
    ensure!(config.max_files > 0, "log.max_files must be positive");
    let (file, guard) = if let Some(directory) = &config.directory {
        std::fs::create_dir_all(directory)?;
        let appender = RollingFileAppender::builder()
            .rotation(rotation)
            .filename_prefix("rosbridge_server_rs.log")
            .max_log_files(config.max_files)
            .build(directory)?;
        let (writer, guard) = tracing_appender::non_blocking(appender);
        (
            Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer),
            ),
            Some(guard),
        )
    } else {
        (None, None)
    };
    let console = config
        .console
        .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(file)
        .try_init()?;
    Ok(guard)
}
