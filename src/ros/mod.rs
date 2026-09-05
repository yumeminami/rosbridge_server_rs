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

//! RCL/RMW backend; every handle remains on the bridge worker thread.
#![allow(unsafe_op_in_unsafe_fn)]
mod message;
use crate::backend::*;
use anyhow::{Context, Result, bail};
use message::{MessageType, ServiceType};
use r2r_rcl::*;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    ffi::{CStr, CString},
    ptr,
    rc::Rc,
    slice,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn check(ret: rcl_ret_t) -> Result<()> {
    if ret == 0 {
        return Ok(());
    }
    unsafe {
        let error = rcutils_get_error_string();
        let text = CStr::from_ptr(error.str_.as_ptr())
            .to_string_lossy()
            .into_owned();
        rcutils_reset_error();
        bail!("ROS error {ret}: {text}")
    }
}

enum Handle {
    Publisher(rcl_publisher_t, Rc<MessageType>),
    Subscription(rcl_subscription_t, Rc<MessageType>),
    Client(rcl_client_t, Rc<ServiceType>),
    Service(rcl_service_t, Rc<ServiceType>),
}

pub struct Ros {
    context: Box<rcl_context_t>,
    node: Box<rcl_node_t>,
    handles: HashMap<Entity, Handle>,
    next: u64,
    requests: HashMap<(Entity, u64), rmw_request_id_t>,
    next_request: u64,
    simulated: Option<(i64, u32)>,
    clock_subscription: Option<Entity>,
}

impl Ros {
    pub fn new(name: &str, namespace: &str, simulated: bool, ros_args: &[String]) -> Result<Self> {
        // Validate C strings before acquiring native resources.
        let name = CString::new(name)?;
        let namespace = CString::new(namespace)?;
        let args = std::iter::once("rosbridge_server_rs")
            .chain(ros_args.iter().map(String::as_str))
            .map(CString::new)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        unsafe {
            let mut context = Box::new(rcl_get_zero_initialized_context());
            let mut options = rcl_get_zero_initialized_init_options();
            check(rcl_init_options_init(
                &mut options,
                rcutils_get_default_allocator(),
            ))?;
            let pointers: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
            let result = check(rcl_init(
                pointers.len() as i32,
                pointers.as_ptr(),
                &options,
                &mut *context,
            ));
            check(rcl_init_options_fini(&mut options))?;
            result?;
            let mut node = Box::new(rcl_get_zero_initialized_node());
            let mut node_options = rcl_node_get_default_options();
            node_options.enable_rosout = false;
            let result = check(rcl_node_init(
                &mut *node,
                name.as_ptr(),
                namespace.as_ptr(),
                &mut *context,
                &node_options,
            ));
            if result.is_err() {
                rcl_shutdown(&mut *context);
                rcl_context_fini(&mut *context);
            }
            result?;
            let mut ros = Self {
                context,
                node,
                handles: HashMap::new(),
                next: 0,
                requests: HashMap::new(),
                next_request: 0,
                simulated: simulated.then_some((0, 0)),
                clock_subscription: None,
            };
            if simulated {
                ros.clock_subscription = Some(ros.subscription(
                    "/clock",
                    "rosgraph_msgs/msg/Clock",
                    Qos::subscriber(),
                )?);
            }
            Ok(ros)
        }
    }

    fn insert(&mut self, handle: Handle) -> Entity {
        self.next += 1;
        self.handles.insert(self.next, handle);
        self.next
    }

    fn graph(&mut self, services: bool) -> Result<BTreeMap<String, Vec<String>>> {
        unsafe {
            let mut names = rmw_get_zero_initialized_names_and_types();
            let mut allocator = rcutils_get_default_allocator();
            let ret = if services {
                rcl_get_service_names_and_types(&*self.node, &mut allocator, &mut names)
            } else {
                rcl_get_topic_names_and_types(&*self.node, &mut allocator, false, &mut names)
            };
            check(ret)?;
            let mut result = BTreeMap::new();
            for i in 0..names.names.size {
                let name = CStr::from_ptr(*names.names.data.add(i))
                    .to_string_lossy()
                    .into_owned();
                let types = &*names.types.add(i);
                let types = (0..types.size)
                    .map(|j| {
                        CStr::from_ptr(*types.data.add(j))
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect();
                result.insert(name, types);
            }
            check(rcl_names_and_types_fini(&mut names))?;
            Ok(result)
        }
    }
}

fn qos(q: Qos) -> rmw_qos_profile_t {
    use rmw_qos_durability_policy_t::*;
    use rmw_qos_history_policy_t::*;
    use rmw_qos_reliability_policy_t::*;
    rmw_qos_profile_t {
        history: match q.history {
            1 => RMW_QOS_POLICY_HISTORY_KEEP_LAST,
            2 => RMW_QOS_POLICY_HISTORY_KEEP_ALL,
            _ => RMW_QOS_POLICY_HISTORY_SYSTEM_DEFAULT,
        },
        depth: q.depth,
        reliability: match q.reliability {
            1 => RMW_QOS_POLICY_RELIABILITY_RELIABLE,
            2 => RMW_QOS_POLICY_RELIABILITY_BEST_EFFORT,
            4 => RMW_QOS_POLICY_RELIABILITY_BEST_AVAILABLE,
            _ => RMW_QOS_POLICY_RELIABILITY_SYSTEM_DEFAULT,
        },
        durability: match q.durability {
            1 => RMW_QOS_POLICY_DURABILITY_TRANSIENT_LOCAL,
            2 => RMW_QOS_POLICY_DURABILITY_VOLATILE,
            4 => RMW_QOS_POLICY_DURABILITY_BEST_AVAILABLE,
            _ => RMW_QOS_POLICY_DURABILITY_SYSTEM_DEFAULT,
        },
        deadline: rmw_time_t {
            sec: q.deadline.0,
            nsec: q.deadline.1,
        },
        lifespan: rmw_time_t {
            sec: q.lifespan.0,
            nsec: q.lifespan.1,
        },
        liveliness: rmw_qos_liveliness_policy_t::RMW_QOS_POLICY_LIVELINESS_SYSTEM_DEFAULT,
        liveliness_lease_duration: rmw_time_t { sec: 0, nsec: 0 },
        avoid_ros_namespace_conventions: false,
    }
}

impl Backend for Ros {
    fn publisher(&mut self, name: &str, typ: &str, q: Qos) -> Result<Entity> {
        let typ = MessageType::load(typ)?;
        unsafe {
            let mut handle = rcl_get_zero_initialized_publisher();
            let mut options = rcl_publisher_get_default_options();
            options.qos = qos(q);
            check(rcl_publisher_init(
                &mut handle,
                &*self.node,
                typ.support,
                CString::new(name)?.as_ptr(),
                &options,
            ))?;
            Ok(self.insert(Handle::Publisher(handle, typ)))
        }
    }

    fn subscription(&mut self, name: &str, typ: &str, q: Qos) -> Result<Entity> {
        let typ = MessageType::load(typ)?;
        unsafe {
            let mut handle = rcl_get_zero_initialized_subscription();
            let mut options = rcl_subscription_get_default_options();
            options.qos = qos(q);
            check(rcl_subscription_init(
                &mut handle,
                &*self.node,
                typ.support,
                CString::new(name)?.as_ptr(),
                &options,
            ))?;
            Ok(self.insert(Handle::Subscription(handle, typ)))
        }
    }

    fn client(&mut self, name: &str, typ: &str) -> Result<Entity> {
        let typ = ServiceType::load(typ)?;
        unsafe {
            let mut handle = rcl_get_zero_initialized_client();
            let options = rcl_client_get_default_options();
            check(rcl_client_init(
                &mut handle,
                &*self.node,
                typ.support,
                CString::new(name)?.as_ptr(),
                &options,
            ))?;
            Ok(self.insert(Handle::Client(handle, typ)))
        }
    }

    fn service(&mut self, name: &str, typ: &str) -> Result<Entity> {
        let typ = ServiceType::load(typ)?;
        unsafe {
            let mut handle = rcl_get_zero_initialized_service();
            let options = rcl_service_get_default_options();
            check(rcl_service_init(
                &mut handle,
                &*self.node,
                typ.support,
                CString::new(name)?.as_ptr(),
                &options,
            ))?;
            Ok(self.insert(Handle::Service(handle, typ)))
        }
    }

    fn publish(&mut self, id: Entity, value: &Value) -> Result<()> {
        let Some(Handle::Publisher(handle, typ)) = self.handles.get(&id) else {
            bail!("publisher no longer exists")
        };
        let mut msg = typ.message();
        msg.fill(value, self.now())?;
        unsafe { check(rcl_publish(handle, msg.data, ptr::null_mut())) }
    }

    fn request(&mut self, id: Entity, value: &Value) -> Result<i64> {
        let Some(Handle::Client(handle, typ)) = self.handles.get(&id) else {
            bail!("service client no longer exists")
        };
        let mut msg = typ.request.message();
        msg.fill(value, self.now())?;
        let mut sequence = 0;
        unsafe {
            check(rcl_send_request(handle, msg.data, &mut sequence))?;
        }
        Ok(sequence)
    }

    fn validate_response(&self, id: Entity, value: &Value) -> Result<()> {
        let Some(Handle::Service(_, typ)) = self.handles.get(&id) else {
            bail!("service no longer exists")
        };
        typ.response.message().fill(value, self.now())
    }

    fn respond(&mut self, id: Entity, request: u64, value: &Value) -> Result<()> {
        let Some(Handle::Service(handle, typ)) = self.handles.get(&id) else {
            bail!("service no longer exists")
        };
        let mut msg = typ.response.message();
        msg.fill(value, self.now())?;
        let mut header = *self
            .requests
            .get(&(id, request))
            .context("unknown request")?;
        unsafe {
            check(rcl_send_response(handle, &mut header, msg.data))?;
        }
        self.requests.remove(&(id, request));
        Ok(())
    }

    fn discard_request(&mut self, entity: Entity, request: u64) {
        self.requests.remove(&(entity, request));
    }

    fn destroy(&mut self, id: Entity) {
        self.requests.retain(|(entity, _), _| *entity != id);
        if let Some(mut handle) = self.handles.remove(&id) {
            unsafe {
                let result = match &mut handle {
                    Handle::Publisher(h, _) => rcl_publisher_fini(h, &mut *self.node),
                    Handle::Subscription(h, _) => rcl_subscription_fini(h, &mut *self.node),
                    Handle::Client(h, _) => rcl_client_fini(h, &mut *self.node),
                    Handle::Service(h, _) => rcl_service_fini(h, &mut *self.node),
                };
                if let Err(e) = check(result) {
                    tracing::warn!("destroy entity: {e}");
                }
            }
        }
    }

    fn poll(&mut self) -> Result<Vec<Event>> {
        let mut events = Vec::new();
        let now = self.now();
        for (&id, handle) in &self.handles {
            unsafe {
                // A per-entity budget prevents a hot camera stream starving requests.
                for _ in 0..32 {
                    match handle {
                        Handle::Subscription(h, t) => {
                            let msg = t.message();
                            let ret = rcl_take(h, msg.data, ptr::null_mut(), ptr::null_mut());
                            if ret == RCL_RET_SUBSCRIPTION_TAKE_FAILED as i32 {
                                break;
                            }
                            check(ret)?;
                            let (json, cbor) = msg.values()?;
                            if Some(id) == self.clock_subscription {
                                self.simulated = Some((
                                    json["clock"]["sec"].as_i64().unwrap_or(0),
                                    json["clock"]["nanosec"].as_u64().unwrap_or(0) as u32,
                                ));
                                continue;
                            }
                            events.push(Event::Message(
                                id,
                                RosMessage {
                                    json,
                                    cbor,
                                    raw: msg.raw()?,
                                    stamp: now,
                                },
                            ));
                        }
                        Handle::Client(h, t) => {
                            let msg = t.response.message();
                            let mut header = std::mem::zeroed();
                            let ret = rcl_take_response(h, &mut header, msg.data);
                            if ret == RCL_RET_CLIENT_TAKE_FAILED as i32 {
                                break;
                            }
                            check(ret)?;
                            events.push(Event::Response {
                                entity: id,
                                sequence: header.sequence_number,
                                values: msg.values()?.0,
                            });
                        }
                        Handle::Service(h, t) => {
                            let msg = t.request.message();
                            let mut header = std::mem::zeroed();
                            let ret = rcl_take_request(h, &mut header, msg.data);
                            if ret == RCL_RET_SERVICE_TAKE_FAILED as i32 {
                                break;
                            }
                            check(ret)?;
                            self.next_request += 1;
                            self.requests.insert((id, self.next_request), header);
                            events.push(Event::Request {
                                entity: id,
                                request: self.next_request,
                                args: msg.values()?.0,
                            });
                        }
                        Handle::Publisher(..) => break,
                    }
                }
            }
        }
        Ok(events)
    }

    fn topics(&mut self) -> Result<BTreeMap<String, Vec<String>>> {
        self.graph(false)
    }

    fn services(&mut self) -> Result<BTreeMap<String, Vec<String>>> {
        self.graph(true)
    }

    fn publisher_qos(&mut self, topic: &str) -> Result<Vec<Qos>> {
        unsafe {
            let mut info = rmw_get_zero_initialized_topic_endpoint_info_array();
            let mut allocator = rcutils_get_default_allocator();
            check(rcl_get_publishers_info_by_topic(
                &*self.node,
                &mut allocator,
                CString::new(topic)?.as_ptr(),
                false,
                &mut info,
            ))?;
            let values = if info.size == 0 {
                Vec::new()
            } else {
                slice::from_raw_parts(info.info_array, info.size)
                    .iter()
                    .map(|i| {
                        let q = i.qos_profile;
                        Qos {
                            history: q.history as u32,
                            depth: q.depth,
                            reliability: q.reliability as u32,
                            durability: q.durability as u32,
                            deadline: (q.deadline.sec, q.deadline.nsec),
                            lifespan: (q.lifespan.sec, q.lifespan.nsec),
                        }
                    })
                    .collect()
            };
            check(rmw_topic_endpoint_info_array_fini(
                &mut info,
                &mut allocator,
            ))?;
            Ok(values)
        }
    }

    fn now(&self) -> (i64, u32) {
        self.simulated.unwrap_or_else(|| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            (now.as_secs() as i64, now.subsec_nanos())
        })
    }
}

impl Drop for Ros {
    fn drop(&mut self) {
        let ids: Vec<_> = self.handles.keys().copied().collect();
        for id in ids {
            self.destroy(id);
        }
        unsafe {
            let _ = check(rcl_node_fini(&mut *self.node));
            let _ = check(rcl_shutdown(&mut *self.context));
            let _ = check(rcl_context_fini(&mut *self.context));
        }
    }
}

#[cfg(test)]
mod message_tests;
