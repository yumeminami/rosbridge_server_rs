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

//! TOML settings and explicit command-line precedence.
use crate::Args;
use anyhow::{Context, Result, ensure};
use clap::{ArgMatches, parser::ValueSource};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    address: Option<String>,
    port: Option<u16>,
    node_name: Option<String>,
    namespace: Option<String>,
    use_sim_time: Option<bool>,
    no_rosapi: Option<bool>,
    default_call_service_timeout: Option<f64>,
    max_message_size: Option<usize>,
    url_path: Option<String>,
    incoming_queue_size: Option<usize>,
    write_queue_size: Option<usize>,
    fragment_timeout: Option<f64>,
    topics_glob: Option<Vec<String>>,
    topics_pub_glob: Option<Vec<String>>,
    topics_sub_glob: Option<Vec<String>>,
    services_glob: Option<Vec<String>>,
    params_glob: Option<Vec<String>>,
    params_timeout: Option<f64>,
    log: Log,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Timezone {
    #[default]
    Local,
    Utc,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Log {
    pub level: String,
    pub directory: Option<std::path::PathBuf>,
    pub rotation: String,
    pub max_files: usize,
    pub console: bool,
    pub timezone: Timezone,
    pub ansi: bool,
}
impl Default for Log {
    fn default() -> Self {
        Self {
            level: "info".into(),
            directory: None,
            rotation: "daily".into(),
            max_files: 7,
            console: true,
            timezone: Timezone::Local,
            ansi: false,
        }
    }
}

fn ensure_default_config(path: &std::path::Path, version: &str) -> Result<()> {
    let directory = path.parent().context("config path has no parent")?;
    let marker = directory.join(".config-version");
    let previous = match std::fs::read_to_string(&marker) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("read default config version"),
    };
    if previous.as_deref() == Some(version) && path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(directory)?;
    std::fs::write(path, include_bytes!("../rosbridge.toml"))
        .with_context(|| format!("refresh default config {}", path.display()))?;
    std::fs::write(marker, version).context("write default config version")?;
    Ok(())
}

pub fn load(args: &mut Args, matches: &ArgMatches) -> Result<Log> {
    if args.config.is_none() {
        let home = std::env::var_os("HOME").context("HOME is unset; provide --config")?;
        let path = std::path::PathBuf::from(home).join(".rosbridge_server_rs/rosbridge.toml");
        ensure_default_config(&path, env!("CARGO_PKG_VERSION"))?;
        args.config = Some(path);
    }
    let config: Config = match &args.config {
        Some(path) => toml::from_str(
            &std::fs::read_to_string(path)
                .with_context(|| format!("read config {}", path.display()))?,
        )
        .with_context(|| format!("parse config {}", path.display()))?,
        None => Config::default(),
    };
    if matches.value_source("bind") != Some(ValueSource::CommandLine) {
        if let Some(address) = config.address {
            args.bind.set_ip(
                if address.is_empty() {
                    "0.0.0.0"
                } else {
                    &address
                }
                .parse()
                .context("address must be an IP address")?,
            );
        }
        if let Some(port) = config.port {
            args.bind.set_port(port);
        }
    }
    macro_rules! apply {
        ($field:ident, $value:expr) => {
            if matches.value_source(stringify!($field)) != Some(ValueSource::CommandLine) {
                if let Some(value) = $value {
                    args.$field = value;
                }
            }
        };
    }
    apply!(node_name, config.node_name);
    apply!(namespace, config.namespace);
    apply!(use_sim_time, config.use_sim_time);
    apply!(no_rosapi, config.no_rosapi);
    apply!(service_timeout, config.default_call_service_timeout);
    apply!(max_message_size, config.max_message_size);
    apply!(url_path, config.url_path);
    apply!(incoming_queue_size, config.incoming_queue_size);
    apply!(write_queue_size, config.write_queue_size);
    apply!(fragment_timeout, config.fragment_timeout);
    ensure!(
        args.incoming_queue_size > 0 && args.write_queue_size > 0,
        "queue sizes must be positive"
    );
    ensure!(
        args.max_message_size > 0,
        "max_message_size must be positive"
    );
    ensure!(
        args.url_path.starts_with('/') && !args.url_path.contains(['?', '#']),
        "url_path must be an absolute URL path"
    );
    for (name, value) in [
        ("fragment_timeout", args.fragment_timeout),
        ("default_call_service_timeout", args.service_timeout),
    ] {
        ensure!(
            value.is_finite() && value > 0.0,
            "{name} must be finite and positive"
        );
    }
    let mut params = Vec::new();
    for (name, patterns) in [
        ("topics_glob", config.topics_glob),
        ("topics_pub_glob", config.topics_pub_glob),
        ("topics_sub_glob", config.topics_sub_glob),
        ("services_glob", config.services_glob),
        ("params_glob", config.params_glob),
    ] {
        if let Some(patterns) = patterns {
            for pattern in &patterns {
                glob::Pattern::new(pattern)?;
            }
            params.extend([
                "-p".into(),
                format!("{name}:={}", serde_json::to_string(&patterns)?),
            ]);
        }
    }
    if let Some(timeout) = config.params_timeout {
        ensure!(
            timeout.is_finite() && timeout > 0.0,
            "params_timeout must be finite and positive"
        );
        params.extend(["-p".into(), format!("params_timeout:={timeout}")]);
    }
    if !params.is_empty() {
        let mut ros = vec!["--ros-args".to_string()];
        ros.extend(params);
        ros.extend(
            args.ros_args
                .iter()
                .filter(|arg| arg.as_str() != "--ros-args")
                .cloned(),
        );
        args.ros_args = ros;
    }
    let mut log = config.log;
    if let Ok(level) = std::env::var("RUST_LOG") {
        log.level = level;
    }
    if let Some(level) = &args.log_level {
        log.level = level.clone();
    }
    if let Some(directory) = &args.log_directory {
        log.directory = Some(directory.clone());
    }
    if let Some(timezone) = args.log_timezone {
        log.timezone = timezone;
    }
    if let Some(ansi) = args.log_ansi {
        log.ansi = ansi;
    }
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, FromArgMatches};

    fn configured(text: &str, flags: &[&str]) -> Result<Args> {
        let path = std::env::temp_dir().join(format!("rosbridge-{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&path, text)?;
        let mut argv = vec!["rosbridge", "--config", path.to_str().unwrap()];
        argv.extend_from_slice(flags);
        let matches = Args::command().try_get_matches_from(argv)?;
        let mut args = Args::from_arg_matches(&matches)?;
        let result = load(&mut args, &matches);
        std::fs::remove_file(path)?;
        result?;
        Ok(args)
    }

    #[test]
    fn default_config_refreshes_on_version_change() {
        let directory = std::env::temp_dir().join(format!("rosbridge-{}", uuid::Uuid::new_v4()));
        let path = directory.join("rosbridge.toml");
        ensure_default_config(&path, "0.1.3").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _: Config = toml::from_str(&text).unwrap();
        assert_eq!(text, include_str!("../rosbridge.toml"));
        std::fs::write(&path, "port = 8443").unwrap();
        ensure_default_config(&path, "0.1.3").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "port = 8443");
        ensure_default_config(&path, "0.1.4").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_file(directory.join(".config-version")).unwrap();
        std::fs::write(&path, "port = 8443").unwrap();
        ensure_default_config(&path, "0.1.3").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_file(&path).unwrap();
        ensure_default_config(&path, "0.1.3").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn command_line_overrides_file() {
        let args = configured(
            "port = 8443\nmax_message_size = 1024\nnamespace = '/soc'",
            &["--bind", "127.0.0.1:9091", "--max-message-size", "2048"],
        )
        .unwrap();
        assert_eq!(args.bind.to_string(), "127.0.0.1:9091");
        assert_eq!(args.max_message_size, 2048);
        assert_eq!(args.namespace, "/soc");
    }
    #[test]
    fn file_settings_and_ros_parameter_order() {
        let args = configured(
            "address = '127.0.0.1'\nport = 8443\nparams_timeout = 2.0",
            &["--", "--ros-args", "-p", "params_timeout:=3.0"],
        )
        .unwrap();
        assert_eq!(args.bind.to_string(), "127.0.0.1:8443");
        assert_eq!(args.ros_args.last().unwrap(), "params_timeout:=3.0");
    }
    #[test]
    fn reject_unknown_or_invalid_settings() {
        for text in [
            "ssl = true",
            "write_queue_size = 0",
            "fragment_timeout = -1",
            "url_path = 'relative'",
        ] {
            assert!(configured(text, &[]).is_err(), "{text}");
        }
    }
}
