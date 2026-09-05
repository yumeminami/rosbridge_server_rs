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
bash scripts/test.sh --junitxml=/work/ci-results/websocket.xml
source /opt/ros/jazzy/setup.bash
source target/rosbridge-test-install/setup.bash
cargo build --locked --release
python3 benchmarks/parity.py
