#!/usr/bin/env bash
#
# Copyright (c) 2026 Wing Mun Fung
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0, available at
# https://www.eclipse.org/legal/epl-2.0/, or the Apache License, Version 2.0,
# available at https://www.apache.org/licenses/LICENSE-2.0.
#
# SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
#

# Source this file from the repository root inside the test container.
source /opt/ros/jazzy/setup.bash
colcon --log-base /tmp/rosbridge-colcon-log build \
    --base-paths /upstream/rosbridge_test_msgs \
    --build-base /tmp/rosbridge-build \
    --install-base /work/target/rosbridge-test-install \
    --cmake-args -DBUILD_TESTING=OFF
source target/rosbridge-test-install/setup.bash
