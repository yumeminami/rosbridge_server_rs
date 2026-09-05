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

# Package a native release build in its supported ROS distribution container.
set -euo pipefail
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
arch=$(dpkg --print-architecture)
case "$ROS_DISTRO" in
    humble) ubuntu=22.04 ;;
    jazzy) ubuntu=24.04 ;;
    *) echo "Unsupported ROS distribution: $ROS_DISTRO" >&2; exit 1 ;;
esac
case "$arch" in
    amd64|arm64) ;;
    *) echo "Unsupported release architecture: $arch" >&2; exit 1 ;;
esac
if [[ ${GITHUB_REF_TYPE:-} == tag && ${GITHUB_REF_NAME} != "v$version" ]]; then
    echo "Release tag must match Cargo.toml: v$version" >&2
    exit 1
fi
output="$PWD/ci-results/dist"
mkdir -p "$output"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
name="rosbridge-server-rs_${version}_${ROS_DISTRO}_ubuntu${ubuntu}_${arch}"
root="$work/$name"
install -Dm755 target/release/rosbridge_server_rs "$root/usr/bin/rosbridge_server_rs"
ln -s rosbridge_server_rs "$root/usr/bin/rosbridge-server-rs"
strip "$root/usr/bin/rosbridge_server_rs"
install -Dm644 LICENSE "$root/usr/share/doc/rosbridge-server-rs/copyright"
install -Dm644 README.md "$root/usr/share/doc/rosbridge-server-rs/README.md"
install -Dm644 rosbridge.toml "$root/usr/share/doc/rosbridge-server-rs/rosbridge.toml"
mkdir -p "$work/debian" "$root/DEBIAN"
cat > "$work/debian/control" <<CONTROL
Source: rosbridge-server-rs
Section: net
Priority: optional
Maintainer: Wing Mun Fung <yumeminami@users.noreply.github.com>

Package: rosbridge-server-rs
Architecture: any
Description: ROS 2 rosbridge WebSocket server written in Rust
CONTROL
# Resolve ELF dependencies from the installed distribution packages. Runtime-loaded
# interface libraries and the default RMW must be declared separately.
depends=$(cd "$work" && dpkg-shlibdeps -O -l"/opt/ros/$ROS_DISTRO/lib" -e"$root/usr/bin/rosbridge_server_rs")
depends=${depends#shlibs:Depends=}
cat > "$root/DEBIAN/control" <<CONTROL
Package: rosbridge-server-rs
Version: $version
Section: net
Priority: optional
Architecture: $arch
Maintainer: Wing Mun Fung <yumeminami@users.noreply.github.com>
Homepage: https://github.com/yumeminami/rosbridge_server_rs
Depends: $depends, ros-$ROS_DISTRO-rcl, ros-$ROS_DISTRO-rcl-action, ros-$ROS_DISTRO-rmw-implementation, ros-$ROS_DISTRO-rosidl-runtime-c, ros-$ROS_DISTRO-rosapi-msgs, ros-$ROS_DISTRO-rcl-interfaces, ros-$ROS_DISTRO-rosgraph-msgs, ros-$ROS_DISTRO-rmw-cyclonedds-cpp | ros-$ROS_DISTRO-rmw-fastrtps-cpp
Description: ROS 2 rosbridge WebSocket server written in Rust
 Implements the rosbridge WebSocket protocol and native rosapi services.
 Requires Ubuntu $ubuntu and ROS 2 $ROS_DISTRO. Source the ROS environment before use.
CONTROL
dpkg-deb --root-owner-group --build "$root" "$output/$name.deb"
tar -czf "$output/$name.tar.gz" \
    -C "$root/usr/bin" rosbridge_server_rs \
    -C "$PWD" LICENSE README.md rosbridge.toml
(cd "$output" && sha256sum "$name.deb" "$name.tar.gz" > "SHA256SUMS-$ROS_DISTRO-$arch")
