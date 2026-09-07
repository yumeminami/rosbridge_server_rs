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

//! Owned protocol encoding jobs and bounded concurrency shared by connections.
use crate::wire::{self, Compression, Options};
use ciborium::Value as Cbor;
use serde_json::Value;
use std::{
    mem::size_of,
    sync::{Arc, LazyLock},
};
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone)]
pub struct Pool(pub(crate) Arc<Semaphore>);
impl Pool {
    pub fn new(workers: usize) -> Self {
        assert!(workers > 0);
        Self(Arc::new(Semaphore::new(workers)))
    }
}
impl Default for Pool {
    fn default() -> Self {
        static DEFAULT: LazyLock<Pool> = LazyLock::new(|| Pool::new(2));
        DEFAULT.clone()
    }
}

pub struct Request {
    value: Value,
    binary: Option<Cbor>,
    options: Options,
    fragment_id: String,
}
impl Request {
    pub fn new(
        mut value: Value,
        mut binary: Option<Cbor>,
        options: Options,
        fragment_id: String,
    ) -> Self {
        if matches!(options.compression, Compression::Cbor | Compression::Raw) {
            if binary.is_some() {
                value = Value::Null;
            }
        } else {
            binary = None;
        }
        Self {
            value,
            binary,
            options,
            fragment_id,
        }
    }
    /// Estimated retained input storage, not wire length or total allocator RSS.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(json_storage(&self.value))
            .saturating_add(self.binary.as_ref().map_or(0, cbor_storage))
            .saturating_add(self.fragment_id.capacity())
    }
    pub fn encode(self) -> anyhow::Result<Vec<Message>> {
        wire::encode(&self.value, self.binary, &self.options, &self.fragment_id)
    }
}
fn sum(values: impl Iterator<Item = usize>) -> usize {
    values.fold(0, usize::saturating_add)
}
fn json_storage(value: &Value) -> usize {
    let heap = match value {
        Value::String(s) => s.capacity(),
        Value::Array(v) => v
            .capacity()
            .saturating_mul(size_of::<Value>())
            .saturating_add(sum(v.iter().map(json_storage))),
        Value::Object(v) => sum(v.iter().map(|(k, v)| {
            k.capacity()
                .saturating_add(size_of::<String>() + 4 * size_of::<usize>())
                .saturating_add(json_storage(v))
        })),
        _ => 0,
    };
    size_of::<Value>().saturating_add(heap)
}
fn cbor_storage(value: &Cbor) -> usize {
    let heap = match value {
        Cbor::Text(s) => s.capacity(),
        Cbor::Bytes(v) => v.capacity(),
        Cbor::Array(v) => v
            .capacity()
            .saturating_mul(size_of::<Cbor>())
            .saturating_add(sum(v.iter().map(cbor_storage))),
        Cbor::Map(v) => v
            .capacity()
            .saturating_mul(size_of::<(Cbor, Cbor)>())
            .saturating_add(sum(v
                .iter()
                .map(|(k, v)| cbor_storage(k).saturating_add(cbor_storage(v))))),
        Cbor::Tag(_, v) => cbor_storage(v),
        _ => 0,
    };
    size_of::<Cbor>().saturating_add(heap)
}
