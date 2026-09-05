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

//! WebSocket payload encoding, decoding, and fragment assembly.

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ciborium::Value as Cbor;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Compression {
    #[default]
    None,
    Png,
    Cbor,
    Raw,
}

impl Compression {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "none" => Ok(Self::None),
            "png" => Ok(Self::Png),
            "cbor" => Ok(Self::Cbor),
            "cbor-raw" => Ok(Self::Raw),
            _ => bail!("unsupported compression {s}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub compression: Compression,
    pub fragment: Option<usize>,
    pub throttle: Duration,
    pub queue: usize,
}

impl Options {
    pub fn parse(v: &Value) -> Result<Self> {
        let integer = |key: &str| -> Result<u64> {
            match v.get(key) {
                None => Ok(0),
                Some(v) => v
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("{key} must be a nonnegative integer")),
            }
        };
        let fragment = integer("fragment_size")? as usize;
        Ok(Self {
            compression: Compression::parse(
                v.get("compression")
                    .map(|v| {
                        v.as_str()
                            .ok_or_else(|| anyhow::anyhow!("compression must be a string"))
                    })
                    .transpose()?
                    .unwrap_or("none"),
            )?,
            fragment: (fragment > 0).then_some(fragment),
            throttle: Duration::from_millis(integer("throttle_rate")?),
            queue: integer("queue_length")? as usize,
        })
    }
}

pub fn cbor_value(v: &Value) -> Cbor {
    match v {
        Value::Null => Cbor::Null,
        Value::Bool(v) => Cbor::Bool(*v),
        Value::String(v) => Cbor::Text(v.clone()),
        Value::Number(n) => {
            if let Some(v) = n.as_i64() {
                Cbor::Integer(v.into())
            } else if let Some(v) = n.as_u64() {
                Cbor::Integer(v.into())
            } else {
                Cbor::Float(n.as_f64().unwrap())
            }
        }
        Value::Array(v) => Cbor::Array(v.iter().map(cbor_value).collect()),
        Value::Object(v) => Cbor::Map(
            v.iter()
                .map(|(k, v)| (Cbor::Text(k.clone()), cbor_value(v)))
                .collect(),
        ),
    }
}

pub fn json_value(v: Cbor) -> Result<Value> {
    Ok(match v {
        Cbor::Null => Value::Null,
        Cbor::Bool(v) => json!(v),
        Cbor::Text(v) => json!(v),
        Cbor::Float(v) => json!(v),
        Cbor::Integer(v) => {
            let n: i128 = v.into();
            if n < 0 {
                json!(i64::try_from(n)?)
            } else {
                json!(u64::try_from(n)?)
            }
        }
        Cbor::Bytes(v) => json!(STANDARD.encode(v)),
        Cbor::Array(v) => Value::Array(v.into_iter().map(json_value).collect::<Result<_>>()?),
        Cbor::Map(v) => Value::Object(
            v.into_iter()
                .map(|(k, v)| {
                    let Cbor::Text(k) = k else {
                        bail!("CBOR object keys must be strings")
                    };
                    Ok((k, json_value(v)?))
                })
                .collect::<Result<_>>()?,
        ),
        Cbor::Tag(tag, v) => typed_array(tag, *v)?,
        _ => bail!("unsupported CBOR value"),
    })
}

fn typed_array(tag: u64, value: Cbor) -> Result<Value> {
    let Cbor::Bytes(bytes) = value else {
        bail!("typed array must contain bytes")
    };
    macro_rules! unpack {
        ($ty:ty) => {{
            ensure!(
                bytes.len() % size_of::<$ty>() == 0,
                "invalid typed array length"
            );
            Value::Array(
                bytes
                    .chunks_exact(size_of::<$ty>())
                    .map(|s| json!(<$ty>::from_le_bytes(s.try_into().unwrap())))
                    .collect(),
            )
        }};
    }
    Ok(match tag {
        64 => json!(STANDARD.encode(bytes)),
        69 => unpack!(u16),
        70 => unpack!(u32),
        71 => unpack!(u64),
        72 => unpack!(i8),
        77 => unpack!(i16),
        78 => unpack!(i32),
        79 => unpack!(i64),
        85 => unpack!(f32),
        86 => unpack!(f64),
        _ => bail!("unsupported CBOR tag {tag}"),
    })
}

pub fn encode(
    value: &Value,
    binary_value: Option<Cbor>,
    options: &Options,
    fragment_id: &str,
) -> Result<Vec<Message>> {
    if matches!(options.compression, Compression::Cbor | Compression::Raw) {
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(
            &binary_value.unwrap_or_else(|| cbor_value(value)),
            &mut bytes,
        )?;
        // Python rosbridge sends binary CBOR without applying fragment_size.
        return Ok(vec![Message::Binary(bytes)]);
    }
    let mut text = serde_json::to_string(value)?;
    if options.compression == Compression::Png {
        let width = ((text.len() as f64 / 3.0).sqrt().floor() as usize).max(1);
        let height = text.len().div_ceil(3 * width);
        let mut rgb = text.as_bytes().to_vec();
        rgb.resize(width * height * 3, b'\n');
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&rgb)?;
        }
        text = serde_json::to_string(&json!({"op":"png","data":STANDARD.encode(bytes)}))?;
    }
    if let Some(limit) = options.fragment.filter(|limit| text.len() > *limit) {
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let mut end = (start + limit).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            ensure!(
                end > start,
                "fragment_size is too small for UTF-8 character"
            );
            chunks.push(&text[start..end]);
            start = end;
        }
        return chunks.iter().enumerate().map(|(i,s)| Ok(Message::Text(serde_json::to_string(&json!({"op":"fragment","id":fragment_id,"data":s,"num":i,"total":chunks.len()}))?))).collect();
    }
    Ok(vec![Message::Text(text)])
}

struct Assembly {
    parts: Vec<Option<String>>,
    bytes: usize,
    created: Instant,
}

#[derive(Default)]
pub struct Decoder {
    fragments: HashMap<String, Assembly>,
}

impl Decoder {
    pub fn decode(&mut self, message: Message, limit: usize) -> Result<Option<Value>> {
        self.fragments
            .retain(|_, a| a.created.elapsed() < Duration::from_secs(30));
        let value = match message {
            Message::Text(text) => {
                ensure!(text.len() <= limit, "message too large");
                serde_json::from_str::<Value>(&text)?
            }
            Message::Binary(bytes) => {
                ensure!(bytes.len() <= limit, "message too large");
                json_value(ciborium::de::from_reader(bytes.as_slice())?)?
            }
            _ => return Ok(None),
        };
        ensure!(value.is_object(), "message must be an object");
        if value["op"] != "fragment" {
            return Ok(Some(value));
        }
        let id = value["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("fragment id is required"))?;
        let total = value["total"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("fragment total is required"))?
            as usize;
        let num = value["num"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("fragment num is required"))? as usize;
        let data = value["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("fragment data is required"))?;
        ensure!(
            total > 0 && total <= 65536 && num < total,
            "invalid fragment indices"
        );
        ensure!(
            self.fragments.len() < 16 || self.fragments.contains_key(id),
            "too many fragment assemblies"
        );
        let a = self
            .fragments
            .entry(id.to_owned())
            .or_insert_with(|| Assembly {
                parts: vec![None; total],
                bytes: 0,
                created: Instant::now(),
            });
        ensure!(a.parts.len() == total, "fragment total changed");
        if a.parts[num].is_none() {
            a.bytes += data.len();
            a.parts[num] = Some(data.to_owned());
        }
        if a.bytes > limit {
            self.fragments.remove(id);
            bail!("assembled message too large");
        }
        if a.parts.iter().all(Option::is_some) {
            let a = self.fragments.remove(id).unwrap();
            let text: String = a.parts.into_iter().flatten().collect();
            let value: Value = serde_json::from_str(&text)?;
            ensure!(
                value.is_object() && value["op"] != "fragment",
                "invalid assembled operation"
            );
            return Ok(Some(value));
        }
        Ok(None)
    }
}
