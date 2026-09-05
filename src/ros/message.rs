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

//! Dynamic ROS C messages. The loaded type-support libraries outlive all pointers
//! into their metadata, and generated init/fini/resize functions own field storage.
#![allow(unsafe_op_in_unsafe_fn)]
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ciborium::Value as Cbor;
use libloading::Library;
use r2r_rcl::*;
use serde_json::{Value, json};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    ffi::{CStr, c_char, c_void},
    ptr,
    rc::Rc,
    slice,
};
type Members = rosidl_typesupport_introspection_c__MessageMembers;
type Member = rosidl_typesupport_introspection_c__MessageMember;

pub struct MessageType {
    pub support: *const rosidl_message_type_support_t,
    members: *const Members,
    _libraries: (Library, Library),
}

impl MessageType {
    pub fn load(name: &str) -> Result<Rc<Self>> {
        let p: Vec<_> = name.split('/').collect();
        ensure!(
            p.len() == 3
                && p.iter()
                    .all(|p| !p.is_empty()
                        && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')),
            "invalid message type {name}"
        );
        unsafe {
            let intro = Library::new(format!(
                "lib{}__rosidl_typesupport_introspection_c.so",
                p[0]
            ))
            .with_context(|| {
                format!("load message definition {name}; install and source its ROS package")
            })?;
            let lib = Library::new(format!("lib{}__rosidl_typesupport_c.so", p[0]))?;
            let suffix = p.join("__");
            type Getter = unsafe extern "C" fn() -> *const rosidl_message_type_support_t;
            let support = lib.get::<Getter>(
                format!("rosidl_typesupport_c__get_message_type_support_handle__{suffix}")
                    .as_bytes(),
            )?();
            let introspection = intro.get::<Getter>(
                format!(
                    "rosidl_typesupport_introspection_c__get_message_type_support_handle__{suffix}"
                )
                .as_bytes(),
            )?();
            ensure!(
                !support.is_null() && !introspection.is_null(),
                "null type support for {name}"
            );
            let members = (*introspection).data.cast::<Members>();
            ensure!(!members.is_null(), "missing type metadata");
            Ok(Rc::new(Self {
                support,
                members,
                _libraries: (intro, lib),
            }))
        }
    }

    pub fn message(self: &Rc<Self>) -> Message {
        unsafe {
            let layout = Layout::from_size_align((*self.members).size_of_.max(1), 16).unwrap();
            let data = alloc_zeroed(layout);
            if data.is_null() {
                handle_alloc_error(layout);
            }
            ((*self.members).init_function.unwrap())(
                data.cast(),
                rosidl_runtime_c__message_initialization::ROSIDL_RUNTIME_C_MSG_INIT_ALL,
            );
            Message {
                typ: self.clone(),
                data: data.cast(),
                layout,
            }
        }
    }
}

pub struct ServiceType {
    pub support: *const rosidl_service_type_support_t,
    pub request: Rc<MessageType>,
    pub response: Rc<MessageType>,
    _library: Library,
}

impl ServiceType {
    pub fn load(name: &str) -> Result<Rc<Self>> {
        let request = MessageType::load(&format!("{name}_Request"))?;
        let response = MessageType::load(&format!("{name}_Response"))?;
        unsafe {
            let lib = Library::new(format!(
                "lib{}__rosidl_typesupport_c.so",
                name.split('/').next().unwrap()
            ))?;
            type Getter = unsafe extern "C" fn() -> *const rosidl_service_type_support_t;
            let support = lib.get::<Getter>(
                format!(
                    "rosidl_typesupport_c__get_service_type_support_handle__{}",
                    name.replace('/', "__")
                )
                .as_bytes(),
            )?();
            ensure!(!support.is_null(), "null service type support");
            Ok(Rc::new(Self {
                support,
                request,
                response,
                _library: lib,
            }))
        }
    }
}

pub struct Message {
    typ: Rc<MessageType>,
    pub data: *mut c_void,
    layout: Layout,
}

impl Drop for Message {
    fn drop(&mut self) {
        unsafe {
            ((*self.typ.members).fini_function.unwrap())(self.data);
            dealloc(self.data.cast(), self.layout);
        }
    }
}

impl Message {
    pub fn fill(&mut self, value: &Value, now: (i64, u32)) -> Result<()> {
        unsafe { write_object(self.typ.members, self.data, value, now, true) }
    }

    pub fn values(&self) -> Result<(Value, Cbor)> {
        unsafe { read_object(self.typ.members, self.data) }
    }

    pub fn raw(&self) -> Result<Vec<u8>> {
        unsafe {
            let mut buffer = rcutils_get_zero_initialized_uint8_array();
            super::check(rcutils_uint8_array_init(
                &mut buffer,
                0,
                &rcutils_get_default_allocator(),
            ))?;
            let result = super::check(rmw_serialize(self.data, self.typ.support, &mut buffer));
            let data = if result.is_ok() && buffer.buffer_length > 0 {
                slice::from_raw_parts(buffer.buffer, buffer.buffer_length).to_vec()
            } else {
                Vec::new()
            };
            let fini = super::check(rcutils_uint8_array_fini(&mut buffer));
            result?;
            fini?;
            Ok(data)
        }
    }
}

unsafe fn fields(m: *const Members) -> &'static [Member] {
    if (*m).member_count_ == 0 {
        &[]
    } else {
        slice::from_raw_parts((*m).members_, (*m).member_count_ as usize)
    }
}

unsafe fn name(p: *const c_char) -> String {
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

unsafe fn nested(m: &Member) -> *const Members {
    (*m.members_).data.cast()
}

unsafe fn message_name(m: *const Members) -> String {
    format!(
        "{}/{}",
        name((*m).message_namespace_).replace("__", "/"),
        name((*m).message_name_)
    )
}

unsafe fn write_object(
    m: *const Members,
    data: *mut c_void,
    value: &Value,
    now: (i64, u32),
    root: bool,
) -> Result<()> {
    let typ = message_name(m);
    let mut value = if typ == "builtin_interfaces/msg/Time" && value.as_str() == Some("now") {
        json!({"sec":now.0,"nanosec":now.1})
    } else if let Some(list) = value.as_array() {
        ensure!(
            list.len() <= fields(m).len(),
            "too many positional arguments for {typ}"
        );
        Value::Object(
            fields(m)
                .iter()
                .zip(list)
                .map(|(f, v)| (name(f.name_), v.clone()))
                .collect(),
        )
    } else {
        value.clone()
    };
    if typ == "builtin_interfaces/msg/Time" {
        if let Some(object) = value.as_object_mut() {
            for (alias, field) in [("secs", "sec"), ("nsecs", "nanosec")] {
                if let Some(value) = object.remove(alias) {
                    object.entry(field).or_insert(value);
                }
            }
        }
    }
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("{typ} requires an object"))?;
    for key in obj.keys() {
        ensure!(
            fields(m).iter().any(|f| name(f.name_) == *key),
            "{typ} has no field {key}"
        );
    }
    for f in fields(m) {
        let key = name(f.name_);
        let p = data.cast::<u8>().add(f.offset_ as usize).cast();
        let header = root
            && key == "header"
            && !f.is_array_
            && f.type_id_ == 18
            && message_name(nested(f)) == "std_msgs/msg/Header";
        let mut v = obj.get(&key).cloned();
        if header {
            let h = v.get_or_insert_with(|| json!({}));
            let h = h
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("header requires an object"))?;
            h.entry("stamp")
                .or_insert_with(|| json!({"sec":now.0,"nanosec":now.1}));
        }
        if let Some(v) = v {
            write_field(f, p, &v, now).with_context(|| format!("field {typ}.{key}"))?;
        }
    }
    Ok(())
}

unsafe fn write_field(f: &Member, p: *mut c_void, v: &Value, now: (i64, u32)) -> Result<()> {
    if !f.is_array_ {
        return write_scalar(f, p, v, now);
    }
    let values = if matches!(f.type_id_, 4 | 8) && v.is_string() {
        STANDARD
            .decode(v.as_str().unwrap())?
            .into_iter()
            .map(|b| json!(b))
            .collect::<Vec<_>>()
    } else {
        v.as_array()
            .ok_or_else(|| anyhow::anyhow!("expected an array"))?
            .clone()
    };
    if f.array_size_ > 0 && !f.is_upper_bound_ {
        ensure!(
            values.len() == f.array_size_,
            "expected array length {}",
            f.array_size_
        );
    } else {
        ensure!(
            f.array_size_ == 0 || values.len() <= f.array_size_,
            "array exceeds bound"
        );
        ensure!(
            (f.resize_function.context("missing array resize function")?)(p, values.len()),
            "failed to resize array"
        );
    }
    let get = f.get_function.context("missing array accessor")?;
    for (i, v) in values.iter().enumerate() {
        write_scalar(f, get(p, i), v, now)?;
    }
    Ok(())
}

unsafe fn write_scalar(f: &Member, p: *mut c_void, v: &Value, now: (i64, u32)) -> Result<()> {
    macro_rules! int {
        ($ty:ty,$method:ident) => {
            ptr::write(
                p.cast::<$ty>(),
                <$ty>::try_from(v.$method().context("expected integer in range")?)?,
            );
        };
    }
    match f.type_id_ {
        1 => {
            let n = v.as_f64().context("expected number")?;
            ensure!(n.abs() <= f32::MAX as f64, "float32 overflow");
            ptr::write(p.cast::<f32>(), n as f32);
        }
        2 => ptr::write(p.cast::<f64>(), v.as_f64().context("expected number")?),
        4 | 7 | 8 => {
            int!(u8, as_u64);
        }
        5 | 10 => {
            int!(u16, as_u64);
        }
        6 => ptr::write(p.cast::<bool>(), v.as_bool().context("expected bool")?),
        9 => {
            int!(i8, as_i64);
        }
        11 => {
            int!(i16, as_i64);
        }
        12 => {
            int!(u32, as_u64);
        }
        13 => {
            int!(i32, as_i64);
        }
        14 => {
            int!(u64, as_u64);
        }
        15 => {
            int!(i64, as_i64);
        }
        16 => {
            let s = v.as_str().context("expected string")?;
            ensure!(
                f.string_upper_bound_ == 0 || s.len() <= f.string_upper_bound_,
                "string exceeds bound"
            );
            ensure!(
                rosidl_runtime_c__String__assignn(p.cast(), s.as_ptr().cast(), s.len()),
                "string allocation failed"
            );
        }
        17 => {
            let s: Vec<_> = v
                .as_str()
                .context("expected string")?
                .encode_utf16()
                .collect();
            ensure!(
                f.string_upper_bound_ == 0 || s.len() <= f.string_upper_bound_,
                "string exceeds bound"
            );
            ensure!(
                rosidl_runtime_c__U16String__assignn(p.cast(), s.as_ptr(), s.len()),
                "string allocation failed"
            );
        }
        18 => write_object(nested(f), p, v, now, false)?,
        _ => bail!("unsupported ROS field type {}", f.type_id_),
    };
    Ok(())
}

unsafe fn read_object(m: *const Members, data: *const c_void) -> Result<(Value, Cbor)> {
    let mut json = serde_json::Map::new();
    let mut cbor = Vec::new();
    for f in fields(m) {
        let key = name(f.name_);
        let p = data.cast::<u8>().add(f.offset_ as usize).cast();
        let (j, c) = read_field(f, p)?;
        json.insert(key.clone(), j);
        cbor.push((Cbor::Text(key), c));
    }
    Ok((Value::Object(json), Cbor::Map(cbor)))
}

unsafe fn read_field(f: &Member, p: *const c_void) -> Result<(Value, Cbor)> {
    if !f.is_array_ {
        return read_scalar(f, p);
    }
    let size = (f.size_function.context("missing array size accessor")?)(p);
    let get = f.get_const_function.context("missing array accessor")?;
    let mut json = Vec::with_capacity(size);
    let mut cbor = Vec::with_capacity(size);
    let mut bytes = Vec::new();
    for i in 0..size {
        let element = get(p, i);
        let (j, c) = read_scalar(f, element)?;
        json.push(j);
        cbor.push(c);
        macro_rules! pack {
            ($ty:ty) => {{
                bytes.extend_from_slice(&ptr::read(element.cast::<$ty>()).to_le_bytes());
            }};
        }
        match f.type_id_ {
            1 => pack!(f32),
            2 => pack!(f64),
            4 | 7 | 8 => pack!(u8),
            9 => pack!(i8),
            10 => pack!(u16),
            11 => pack!(i16),
            12 => pack!(u32),
            13 => pack!(i32),
            14 => pack!(u64),
            15 => pack!(i64),
            _ => {}
        }
    }
    let binary = matches!(f.type_id_, 4 | 8);
    let json = if binary {
        json!(STANDARD.encode(&bytes))
    } else {
        Value::Array(json)
    };
    let tag = match f.type_id_ {
        1 => Some(85),
        2 => Some(86),
        7 | 9 => Some(72),
        10 => Some(69),
        11 => Some(77),
        12 => Some(70),
        13 => Some(78),
        14 => Some(71),
        15 => Some(79),
        _ => None,
    };
    let cbor = if binary {
        Cbor::Bytes(bytes)
    } else if f.array_size_ == 0 || f.is_upper_bound_ {
        tag.map(|tag| Cbor::Tag(tag, Box::new(Cbor::Bytes(bytes))))
            .unwrap_or(Cbor::Array(cbor))
    } else {
        Cbor::Array(cbor)
    };
    Ok((json, cbor))
}

unsafe fn read_scalar(f: &Member, p: *const c_void) -> Result<(Value, Cbor)> {
    macro_rules! read {
        ($ty:ty) => {
            json!(ptr::read(p.cast::<$ty>()))
        };
    }
    let value = match f.type_id_ {
        1 => read!(f32),
        2 => read!(f64),
        4 | 7 | 8 => read!(u8),
        5 | 10 => read!(u16),
        6 => read!(bool),
        9 => read!(i8),
        11 => read!(i16),
        12 => read!(u32),
        13 => read!(i32),
        14 => read!(u64),
        15 => read!(i64),
        16 => {
            let s = &*p.cast::<rosidl_runtime_c__String>();
            json!(if s.size == 0 {
                String::new()
            } else {
                String::from_utf8(slice::from_raw_parts(s.data.cast(), s.size).to_vec())?
            })
        }
        17 => {
            let s = &*p.cast::<rosidl_runtime_c__U16String>();
            json!(if s.size == 0 {
                String::new()
            } else {
                String::from_utf16(slice::from_raw_parts(s.data, s.size))?
            })
        }
        18 => return read_object(nested(f), p),
        _ => bail!("unsupported ROS field type {}", f.type_id_),
    };
    let cbor = match f.type_id_ {
        1 => Cbor::Float(ptr::read(p.cast::<f32>()) as f64),
        2 => Cbor::Float(ptr::read(p.cast::<f64>())),
        _ => crate::wire::cbor_value(&value),
    };
    Ok((value, cbor))
}
