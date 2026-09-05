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

use super::{Handle, Ros, check};
use crate::backend::Entity;
use anyhow::Result;
use r2r_rcl::*;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Only triggering crosses threads. The lock excludes guard destruction.
#[derive(Clone, Default)]
pub struct Wake(Arc<Mutex<Option<usize>>>);
impl Wake {
    pub fn trigger(&self) {
        let guard = self.0.lock().unwrap();
        if let Some(pointer) = *guard {
            unsafe {
                let _ = check(rcl_trigger_guard_condition(
                    pointer as *mut rcl_guard_condition_t,
                ));
            }
        }
    }
}
pub(super) struct Wait {
    set: rcl_wait_set_t,
    guard: Box<rcl_guard_condition_t>,
    wake: Wake,
    dimensions: (usize, usize, usize),
    ready: HashSet<Entity>,
}
impl Wait {
    pub(super) fn new(context: &mut rcl_context_t) -> Result<Self> {
        unsafe {
            let mut guard = Box::new(rcl_get_zero_initialized_guard_condition());
            check(rcl_guard_condition_init(
                &mut *guard,
                context,
                rcl_guard_condition_get_default_options(),
            ))?;
            let mut set = rcl_get_zero_initialized_wait_set();
            if let Err(error) = check(rcl_wait_set_init(
                &mut set,
                0,
                1,
                0,
                0,
                0,
                0,
                context,
                rcutils_get_default_allocator(),
            )) {
                rcl_guard_condition_fini(&mut *guard);
                return Err(error);
            }
            let wake = Wake::default();
            *wake.0.lock().unwrap() = Some((&*guard as *const rcl_guard_condition_t) as usize);
            Ok(Self {
                set,
                guard,
                wake,
                dimensions: (0, 0, 0),
                ready: HashSet::new(),
            })
        }
    }
    pub(super) fn ready(&self, id: Entity) -> bool {
        self.ready.contains(&id)
    }
}
impl Drop for Wait {
    fn drop(&mut self) {
        let mut pointer = self.wake.0.lock().unwrap();
        *pointer = None;
        unsafe {
            let _ = check(rcl_wait_set_fini(&mut self.set));
            let _ = check(rcl_guard_condition_fini(&mut *self.guard));
        }
    }
}
impl Ros {
    pub fn wake_handle(&self) -> Wake {
        self.wait.as_ref().unwrap().wake.clone()
    }
    pub fn wait(&mut self, timeout: Duration) -> Result<()> {
        let wait = self.wait.as_mut().unwrap();
        let mut dimensions = (0, 0, 0);
        for handle in self.handles.values() {
            match handle {
                Handle::Subscription(..) => dimensions.0 += 1,
                Handle::Client(..) => dimensions.1 += 1,
                Handle::Service(..) => dimensions.2 += 1,
                _ => {}
            }
        }
        unsafe {
            if dimensions != wait.dimensions {
                check(rcl_wait_set_resize(
                    &mut wait.set,
                    dimensions.0,
                    1,
                    0,
                    dimensions.1,
                    dimensions.2,
                    0,
                ))?;
                wait.dimensions = dimensions;
            }
            check(rcl_wait_set_clear(&mut wait.set))?;
            check(rcl_wait_set_add_guard_condition(
                &mut wait.set,
                &*wait.guard,
                std::ptr::null_mut(),
            ))?;
            let mut indices = Vec::with_capacity(dimensions.0 + dimensions.1 + dimensions.2);
            for (&id, handle) in &self.handles {
                let mut index = 0;
                let kind = match handle {
                    Handle::Subscription(h, _) => {
                        check(rcl_wait_set_add_subscription(&mut wait.set, h, &mut index))?;
                        0
                    }
                    Handle::Client(h, _) => {
                        check(rcl_wait_set_add_client(&mut wait.set, h, &mut index))?;
                        1
                    }
                    Handle::Service(h, _) => {
                        check(rcl_wait_set_add_service(&mut wait.set, h, &mut index))?;
                        2
                    }
                    _ => continue,
                };
                indices.push((id, kind, index));
            }
            wait.ready.clear();
            let ret = rcl_wait(
                &mut wait.set,
                timeout.as_nanos().min(i64::MAX as u128) as i64,
            );
            if ret == RCL_RET_TIMEOUT as i32 {
                return Ok(());
            }
            check(ret)?;
            for (id, kind, index) in indices {
                let ready = match kind {
                    0 => !(*wait.set.subscriptions.add(index)).is_null(),
                    1 => !(*wait.set.clients.add(index)).is_null(),
                    _ => !(*wait.set.services.add(index)).is_null(),
                };
                if ready {
                    wait.ready.insert(id);
                }
            }
        }
        Ok(())
    }
}
