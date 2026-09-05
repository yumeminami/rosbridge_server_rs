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

# Run build, tests and packaging in one reusable ROS container. Failed downloads
# and compilation remain in the container and named volumes for the next run.
set -euo pipefail
distro=${1:?usage: scripts/ci_container.sh humble|jazzy}
case "$distro" in
    humble|jazzy) ;;
    *) echo "Unsupported ROS distribution: $distro" >&2; exit 1 ;;
esac

name="rosbridge-${distro}-${CI_CONTAINER_SUFFIX:-dev}"
cargo_volume="${name}-cargo"
rustup_volume="${name}-rustup"
target_volume="${name}-target"
ros_image="ros:${distro}-ros-base"
rust_image="rust:1.93-slim-bookworm"
upstream=${ROSBRIDGE_SUITE_PATH:-"$(cd .. && pwd)/rosbridge_suite"}

docker pull "$ros_image"
docker pull "$rust_image"
docker volume create "$cargo_volume" >/dev/null
docker volume create "$rustup_volume" >/dev/null
docker volume create "$target_volume" >/dev/null
docker run --rm -v "$cargo_volume:/cache" "$rust_image" \
    sh -c 'test -x /cache/bin/cargo || cp -a /usr/local/cargo/. /cache/'
docker run --rm -v "$rustup_volume:/cache" "$rust_image" \
    sh -c 'test -d /cache/toolchains || cp -a /usr/local/rustup/. /cache/'

if ! docker container inspect "$name" >/dev/null 2>&1; then
    docker run -d --name "$name" \
        -v "$PWD:/work" \
        -v "$upstream:/upstream:ro" \
        -v "$cargo_volume:/usr/local/cargo" \
        -v "$rustup_volume:/usr/local/rustup" \
        -v "$target_volume:/work/target" \
        -e CARGO_HOME=/usr/local/cargo \
        -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        -e RUSTUP_HOME=/usr/local/rustup \
        -e ROS_DOMAIN_ID="${ROS_DOMAIN_ID:-81}" \
        -e ROSBRIDGE_TEST_RESULTS=/work/ci-results/upstream \
        -w /work "$ros_image" sleep infinity >/dev/null
fi
docker start "$name" >/dev/null

docker exec "$name" bash -lec "
    apt-get -o Acquire::Retries=5 update
    DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=5 install -y --no-install-recommends \\
        clang libclang-dev build-essential pkg-config libssl-dev \\
        ros-$distro-example-interfaces ros-$distro-test-msgs ros-$distro-rosapi \\
        python3-pip python3-websockets python3-pytest python3-cbor2 python3-pil \\
        python3-autobahn python3-twisted \\
        ros-$distro-rosbridge-server ros-$distro-launch-testing
    if [ "$distro" = humble ]; then python3 -m pip install websockets==11.0.3; fi
"
docker exec \
    -e ROSBRIDGE_TEST_RESULTS=/work/ci-results/upstream \
    "$name" bash scripts/ci_ros.sh
docker exec \
    -e GITHUB_REF_TYPE="${GITHUB_REF_TYPE:-}" \
    -e GITHUB_REF_NAME="${GITHUB_REF_NAME:-}" \
    "$name" bash scripts/package.sh
docker exec -e PACKAGE_DIR=/work/ci-results/dist "$name" bash scripts/test_package.sh
