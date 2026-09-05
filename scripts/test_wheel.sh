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

set -eo pipefail
source "/opt/ros/$ROS_DISTRO/setup.bash"
uvx --no-index --find-links /wheel rosbridge-server-rs --version
uv tool install --no-index --find-links /wheel rosbridge-server-rs
"$UV_TOOL_BIN_DIR/rosbridge_server_rs" --version
"$UV_TOOL_BIN_DIR/rosbridge-server-rs" --version
