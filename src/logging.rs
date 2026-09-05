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
use crate::config::{Log, Timezone};
use anyhow::{Result, bail, ensure};
use std::io::IsTerminal;
mod file;
use file::{LogFile, Rotation};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter,
    fmt::{
        format::Writer,
        time::{ChronoLocal, ChronoUtc, FormatTime},
    },
    prelude::*,
};

struct PlainFields;
impl<'writer> tracing_subscriber::fmt::FormatFields<'writer> for PlainFields {
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        tracing_subscriber::fmt::format::DefaultFields::new()
            .format_fields(Writer::new(&mut writer), fields)
    }
}

pub struct Guard {
    worker: Option<WorkerGuard>,
    closed: std::sync::mpsc::Receiver<()>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        drop(self.worker.take());
        // WorkerGuard drains the queue but does not join the writer thread.
        if self
            .closed
            .recv_timeout(std::time::Duration::from_secs(2))
            .is_err()
        {
            eprintln!("timed out finalizing the active log file");
        }
    }
}

#[derive(Clone)]
enum Timer {
    Local(ChronoLocal),
    Utc(ChronoUtc),
}
impl FormatTime for Timer {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        match self {
            Self::Local(timer) => timer.format_time(writer),
            Self::Utc(timer) => timer.format_time(writer),
        }
    }
}

pub fn init(config: &Log) -> Result<Option<Guard>> {
    let filter = EnvFilter::try_new(&config.level)?;
    let timer = match config.timezone {
        Timezone::Local => Timer::Local(ChronoLocal::rfc_3339()),
        Timezone::Utc => Timer::Utc(ChronoUtc::rfc_3339()),
    };
    ensure!(
        config.console || config.directory.is_some(),
        "enable console logging or set log.directory"
    );
    let rotation = match config.rotation.as_str() {
        "daily" => Rotation::Daily,
        "hourly" => Rotation::Hourly,
        "never" => Rotation::Never,
        _ => bail!("log.rotation must be daily, hourly or never"),
    };
    ensure!(config.max_files > 0, "log.max_files must be positive");
    let (file, guard) = if let Some(directory) = &config.directory {
        let (appender, closed) =
            LogFile::new(directory, rotation, config.timezone, config.max_files)?;
        let (writer, guard) = tracing_appender::non_blocking(appender);
        (
            Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_timer(timer.clone())
                    .with_writer(writer),
            ),
            Some(Guard {
                worker: Some(guard),
                closed,
            }),
        )
    } else {
        (None, None)
    };
    let console = config.console.then(|| {
        tracing_subscriber::fmt::layer()
            .with_ansi(config.ansi && std::io::stderr().is_terminal())
            .fmt_fields(PlainFields)
            .with_timer(timer.clone())
            .with_writer(std::io::stderr)
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(file)
        .try_init()?;
    if tracing::enabled!(target: "rosbridge_server_rs::service_payload", tracing::Level::DEBUG) {
        tracing::warn!(
            "DEBUG service payload logging is enabled; payloads may contain sensitive data (4096-byte previews)"
        );
    }
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn level_colors_do_not_bold_fields() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let capture = Capture(output.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(true)
            .fmt_fields(PlainFields)
            .with_writer(move || capture.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(connection = 7, topic = "/imu", "Subscribed");
            tracing::warn!(connection = 7, "Timeout");
            tracing::error!(connection = 7, "Failed");
        });
        let text = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        for code in ["\x1b[32m", "\x1b[33m", "\x1b[31m"] {
            assert!(text.contains(code));
        }
        assert!(text.contains("connection=7"));
        assert!(!text.contains("\x1b[1m"));
    }
}
