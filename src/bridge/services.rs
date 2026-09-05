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

//! Service advertisements, request correlation, and responses.

use super::{Bridge, Call, Connection, ExternalRequest, Service, id, name, required};
use crate::{
    backend::{Backend, Entity, type_name},
    wire::Options,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

impl<B: Backend> Bridge<B> {
    pub(super) fn call_service(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let service = name(v, "service")?;
        let options = Options::parse(v)?;
        let typ = self.resolve(v, "service", "srv")?;
        self.access.parameter_service(&typ, &v["args"])?;
        let timeout = if let Some(t) = v.get("timeout") {
            Duration::try_from_secs_f64(t.as_f64().context("timeout must be a number")?)?
        } else {
            self.timeout
        };
        let entity = self.backend.client(&service, &typ)?;
        let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
        let sequence = match self.backend.request(entity, &args) {
            Ok(s) => s,
            Err(e) => {
                self.backend.destroy(entity);
                return Err(e);
            }
        };
        self.calls.insert(
            (entity, sequence),
            Call {
                parameter_names: typ == "rosapi_msgs/srv/GetParamNames",
                owner,
                id: id(v)?,
                service,
                options,
                expires: Instant::now() + timeout,
            },
        );
        Ok(())
    }

    pub(super) fn advertise_service(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let service = name(v, "service")?;
        let typ = type_name(required(v, "type")?, "srv")?;
        self.access.advertise_service(&typ)?;
        if let Some(s) = self.services.get(&service) {
            ensure!(s.owner == owner, "service is advertised by another client");
            if s.typ == typ {
                return Ok(());
            }
            self.remove_service(owner, &service)?;
        }
        let entity = self.backend.service(&service, &typ)?;
        self.services.insert(
            service,
            Service {
                owner,
                entity,
                typ,
                next_request: 0,
            },
        );
        Ok(())
    }

    pub(super) fn remove_service(&mut self, owner: Connection, name: &str) -> Result<()> {
        ensure!(
            self.services.get(name).is_some_and(|s| s.owner == owner),
            "service is not advertised by this client"
        );
        let s = self.services.remove(name).unwrap();
        self.backend.destroy(s.entity);
        self.external.retain(|_, r| r.entity != s.entity);
        Ok(())
    }

    pub(super) fn service_response(&mut self, owner: Connection, v: &Value) -> Result<()> {
        let id = required(v, "id")?;
        let service = name(v, "service")?;
        let r = self
            .external
            .get(id)
            .context("unknown service request id")?;
        ensure!(
            r.owner == owner && r.service == service,
            "service response does not belong to this client"
        );
        ensure!(
            v["result"].as_bool().context("result must be boolean")?,
            "ROS services cannot carry an error response; request will time out"
        );
        self.backend.respond(r.entity, r.request, &v["values"])?;
        self.external.remove(id);
        Ok(())
    }

    pub(super) fn service_request(
        &mut self,
        entity: Entity,
        request: u64,
        args: Value,
    ) -> Result<()> {
        if self.action_request(entity, request, args.clone())? {
            return Ok(());
        }
        if let Some((name, s)) = self.services.iter_mut().find(|(_, s)| s.entity == entity) {
            let name = name.clone();
            let owner = s.owner;
            s.next_request += 1;
            let id = format!("service_request:{name}:{}", s.next_request);
            self.external.insert(
                id.clone(),
                ExternalRequest {
                    owner,
                    entity,
                    request,
                    service: name.clone(),
                    expires: Instant::now() + self.timeout,
                },
            );
            self.send(
                owner,
                json!({"op":"call_service","service":name,"args":args}),
                &Some(id),
                &Options::default(),
                None,
            );
        }
        Ok(())
    }
}
