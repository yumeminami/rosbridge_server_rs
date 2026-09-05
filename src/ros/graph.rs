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

use super::{Ros, check};
use anyhow::Result;
use r2r_rcl::*;
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
};
pub(super) fn public(name: &str) -> bool {
    !name.split('/').any(|part| part.starts_with('_'))
}
unsafe fn strings(array: &rcutils_string_array_t) -> Vec<String> {
    (0..array.size)
        .map(|i| unsafe {
            CStr::from_ptr(*array.data.add(i))
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}
impl Ros {
    pub(super) fn nodes(&self) -> Result<Vec<String>> {
        unsafe {
            let mut names = rcutils_get_zero_initialized_string_array();
            let mut namespaces = rcutils_get_zero_initialized_string_array();
            check(rcl_get_node_names(
                &*self.node,
                rcutils_get_default_allocator(),
                &mut names,
                &mut namespaces,
            ))?;
            let result = strings(&names)
                .into_iter()
                .zip(strings(&namespaces))
                .map(|(name, ns)| format!("{}/{name}", ns.trim_end_matches('/')))
                .filter(|name| public(name))
                .collect();
            check(rcutils_string_array_fini(&mut names))?;
            check(rcutils_string_array_fini(&mut namespaces))?;
            Ok(result)
        }
    }
    pub(super) fn node_graph(
        &self,
        full_name: &str,
        kind: &str,
    ) -> Result<BTreeMap<String, Vec<String>>> {
        let full_name = format!("/{}", full_name.trim_start_matches('/'));
        let (namespace, name) = full_name.rsplit_once('/').unwrap();
        let name = CString::new(name)?;
        let namespace = CString::new(if namespace.is_empty() { "/" } else { namespace })?;
        unsafe {
            let mut names = rmw_get_zero_initialized_names_and_types();
            let mut allocator = rcutils_get_default_allocator();
            let ret = match kind {
                "publishers" => rcl_get_publisher_names_and_types_by_node(
                    &*self.node,
                    &mut allocator,
                    false,
                    name.as_ptr(),
                    namespace.as_ptr(),
                    &mut names,
                ),
                "subscribers" => rcl_get_subscriber_names_and_types_by_node(
                    &*self.node,
                    &mut allocator,
                    false,
                    name.as_ptr(),
                    namespace.as_ptr(),
                    &mut names,
                ),
                _ => rcl_get_service_names_and_types_by_node(
                    &*self.node,
                    &mut allocator,
                    name.as_ptr(),
                    namespace.as_ptr(),
                    &mut names,
                ),
            };
            check(ret)?;
            let result = strings(&names.names)
                .into_iter()
                .enumerate()
                .filter(|(_, name)| public(name))
                .map(|(i, name)| (name, strings(&*names.types.add(i))))
                .collect();
            check(rcl_names_and_types_fini(&mut names))?;
            Ok(result)
        }
    }
}
