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

# Run inside a fresh ROS core container with /dist and /tests mounted.
set -eo pipefail
package_dir=${PACKAGE_DIR:-/dist}
apt-get -o Acquire::Retries=3 update
apt-get -o Acquire::Retries=3 install -y --no-install-recommends \
    "$package_dir"/*_"$ROS_DISTRO"_*.deb python3-websockets
source "/opt/ros/$ROS_DISTRO/setup.bash"
export ROS_DOMAIN_ID=87 ROS_LOCALHOST_ONLY=1
rosbridge_server_rs --version
rosbridge-server-rs --version
python3 "$(dirname "$0")/../tests/test_package.py" /usr/bin/rosbridge_server_rs
# Verify the archive also runs without the development workspace.
mkdir -p /tmp/rosbridge-archive
tar -xzf "$package_dir"/*_"$ROS_DISTRO"_*.tar.gz -C /tmp/rosbridge-archive
python3 "$(dirname "$0")/../tests/test_package.py" /tmp/rosbridge-archive/rosbridge_server_rs
apt-get remove -y rosbridge-server-rs
test ! -e /usr/bin/rosbridge_server_rs
