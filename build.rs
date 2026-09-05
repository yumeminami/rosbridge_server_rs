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

fn main() {
    println!("cargo:rerun-if-env-changed=ROS_DISTRO");
    println!("cargo:rustc-check-cfg=cfg(ros_humble)");
    if std::env::var("ROS_DISTRO").as_deref() == Ok("humble") {
        println!("cargo:rustc-cfg=ros_humble");
    }
}
