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

//! Register shutdown signals before starting the server so cleanup can run normally.

pub(crate) struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl Signals {
    pub(crate) fn new() -> std::io::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            interrupt: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
            #[cfg(unix)]
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        })
    }

    pub(crate) async fn recv(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.interrupt.recv() => {},
            _ = self.terminate.recv() => {},
        }
        #[cfg(not(unix))]
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "Failed to receive shutdown signal");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // Run the real signal handler and logging guard in an isolated process.
    #[test]
    fn signal_child() {
        let Some(directory) = std::env::var_os("ROSBRIDGE_SIGNAL_TEST_DIR") else {
            return;
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut signals = Signals::new().unwrap();
            let config = crate::config::Log {
                directory: Some(directory.into()),
                console: false,
                ..Default::default()
            };
            let _guard = crate::logging::init(&config).unwrap();
            tracing::info!("before signal");
            std::fs::write(config.directory.as_ref().unwrap().join("ready"), b"").unwrap();
            signals.recv().await;
            tracing::info!("after signal");
        });
    }

    #[test]
    fn signals_drain_and_archive_logs() {
        use std::{
            fs,
            process::Command,
            time::{Duration, Instant},
        };
        for signal in ["-INT", "-TERM"] {
            let directory =
                std::env::temp_dir().join(format!("rosbridge-signal-{}", uuid::Uuid::new_v4()));
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "shutdown::tests::signal_child", "--nocapture"])
                .env("ROSBRIDGE_SIGNAL_TEST_DIR", &directory)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !directory.join("ready").exists() {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "child exited before ready"
                );
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("child did not become ready");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                Command::new("kill")
                    .args([signal, &child.id().to_string()])
                    .status()
                    .unwrap()
                    .success()
            );
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("child did not shut down");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let paths: Vec<_> = fs::read_dir(&directory)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert!(
                !paths
                    .iter()
                    .any(|p| p.extension().is_some_and(|e| e == "logging"))
            );
            let archives: Vec<_> = paths
                .iter()
                .filter(|p| p.extension().is_some_and(|e| e == "log"))
                .collect();
            assert_eq!(archives.len(), 1);
            let text = fs::read_to_string(archives[0]).unwrap();
            assert!(text.contains("before signal"));
            assert!(text.contains("after signal"));
            fs::remove_dir_all(directory).unwrap();
        }
    }
}
