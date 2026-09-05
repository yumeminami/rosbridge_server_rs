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

//! Timestamped active files and completed archives for the background log writer.

use crate::config::Timezone;
use chrono::{DateTime, FixedOffset, Local, Utc};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub(super) enum Rotation {
    Daily,
    Hourly,
    Never,
}

pub(super) struct LogFile {
    file: File,
    path: PathBuf,
    directory: PathBuf,
    rotation: Rotation,
    timezone: Timezone,
    period: String,
    max_files: usize,
    closed: Option<std::sync::mpsc::Sender<()>>,
}

fn now(timezone: Timezone) -> DateTime<FixedOffset> {
    match timezone {
        Timezone::Local => Local::now().fixed_offset(),
        Timezone::Utc => Utc::now().fixed_offset(),
    }
}

impl Rotation {
    fn period(self, time: DateTime<FixedOffset>) -> String {
        match self {
            Self::Daily => time.format("%Y%m%d").to_string(),
            Self::Hourly => time.format("%Y%m%d%H%z").to_string(),
            Self::Never => String::new(),
        }
    }
}

fn create(directory: &Path, time: DateTime<FixedOffset>) -> io::Result<(File, PathBuf)> {
    let timestamp = time.format("%Y%m%d%H%M").to_string();
    for index in 0_u64.. {
        let stem = if index == 0 {
            timestamp.clone()
        } else {
            format!("{timestamp}-{index}")
        };
        let path = directory.join(format!("{stem}.logging"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                if path.with_extension("log").exists() {
                    drop(file);
                    fs::remove_file(path)?;
                    continue;
                }
                return Ok((file, path));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

fn is_archive(path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("log") {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let (stamp, suffix) = stem
        .split_once('-')
        .map_or((stem, None), |(a, b)| (a, Some(b)));
    stamp.len() == 12
        && stamp.bytes().all(|b| b.is_ascii_digit())
        && suffix.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

impl LogFile {
    pub(super) fn new(
        directory: &Path,
        rotation: Rotation,
        timezone: Timezone,
        max_files: usize,
    ) -> io::Result<(Self, std::sync::mpsc::Receiver<()>)> {
        let mut writer = Self::at(directory, rotation, timezone, max_files, now(timezone))?;
        let (sender, receiver) = std::sync::mpsc::channel();
        writer.closed = Some(sender);
        Ok((writer, receiver))
    }

    fn at(
        directory: &Path,
        rotation: Rotation,
        timezone: Timezone,
        max_files: usize,
        time: DateTime<FixedOffset>,
    ) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let (file, path) = create(directory, time)?;
        let writer = Self {
            file,
            path,
            directory: directory.to_owned(),
            rotation,
            timezone,
            period: rotation.period(time),
            max_files,
            closed: None,
        };
        writer.prune()?;
        Ok(writer)
    }

    fn archive(&mut self) -> io::Result<()> {
        self.file.flush()?;
        // Unlike rename on Unix, hard_link cannot overwrite an existing archive.
        fs::hard_link(&self.path, self.path.with_extension("log"))?;
        fs::remove_file(&self.path)
    }

    fn prune(&self) -> io::Result<()> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && is_archive(&entry.path()) {
                files.push((entry.metadata()?.modified()?, entry.path()));
            }
        }
        files.sort();
        let excess = files.len().saturating_sub(self.max_files);
        for (_, path) in files.into_iter().take(excess) {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn rotate(&mut self, time: DateTime<FixedOffset>) -> io::Result<()> {
        let period = self.rotation.period(time);
        if period == self.period {
            return Ok(());
        }
        let (file, path) = create(&self.directory, time)?;
        if let Err(error) = self.archive() {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error);
        }
        self.file = file;
        self.path = path;
        self.period = period;
        self.prune()
    }
}

impl Write for LogFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.rotate(now(self.timezone))?;
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Drop for LogFile {
    fn drop(&mut self) {
        if let Err(error) = self.archive().and_then(|()| self.prune()) {
            eprintln!("finalize log {}: {error}", self.path.display());
        }
        if let Some(sender) = self.closed.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_archives_and_restart_does_not_overwrite() {
        let directory =
            std::env::temp_dir().join(format!("rosbridge-log-{}", uuid::Uuid::new_v4()));
        let time = DateTime::parse_from_rfc3339("2026-09-06T02:40:00+08:00").unwrap();
        let next = DateTime::parse_from_rfc3339("2026-09-06T03:00:00+08:00").unwrap();
        let mut log = LogFile::at(&directory, Rotation::Hourly, Timezone::Local, 7, time).unwrap();
        log.file.write_all(b"first").unwrap();
        assert!(directory.join("202609060240.logging").exists());
        log.rotate(next).unwrap();
        assert_eq!(
            fs::read(directory.join("202609060240.log")).unwrap(),
            b"first"
        );
        assert!(directory.join("202609060300.logging").exists());
        drop(log);
        let log = LogFile::at(&directory, Rotation::Never, Timezone::Local, 7, next).unwrap();
        assert_eq!(log.path.file_name().unwrap(), "202609060300-1.logging");
        drop(log);
        assert!(directory.join("202609060300-1.log").exists());
        fs::write(directory.join("unrelated.log"), b"keep").unwrap();
        fs::write(directory.join("202609060301.logging"), b"unfinished").unwrap();
        let log = LogFile::at(&directory, Rotation::Daily, Timezone::Utc, 1, next).unwrap();
        drop(log);
        assert_eq!(
            fs::read_dir(&directory)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| is_archive(&e.path()))
                .count(),
            1
        );
        assert!(directory.join("unrelated.log").exists());
        assert!(directory.join("202609060301.logging").exists());
        assert_eq!(Rotation::Daily.period(time), Rotation::Daily.period(next));
        assert_eq!(Rotation::Never.period(time), Rotation::Never.period(next));
        fs::remove_dir_all(directory).unwrap();
    }
}
