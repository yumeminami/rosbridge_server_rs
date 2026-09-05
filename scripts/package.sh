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

# Package the native release build on Ubuntu 24.04 with ROS 2 Jazzy installed.
set -euo pipefail
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
arch=$(dpkg --print-architecture)
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
name="rosbridge-server-rs_${version}_ubuntu24.04_${arch}"
root="$work/$name"
install -Dm755 target/release/rosbridge_server_rs "$root/usr/bin/rosbridge_server_rs"
strip "$root/usr/bin/rosbridge_server_rs"
install -Dm644 LICENSE "$root/usr/share/doc/rosbridge-server-rs/copyright"
install -Dm644 README.md "$root/usr/share/doc/rosbridge-server-rs/README.md"
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
depends=$(cd "$work" && dpkg-shlibdeps -O -l/opt/ros/jazzy/lib -e"$root/usr/bin/rosbridge_server_rs")
depends=${depends#shlibs:Depends=}
cat > "$root/DEBIAN/control" <<CONTROL
Package: rosbridge-server-rs
Version: $version
Section: net
Priority: optional
Architecture: $arch
Maintainer: Wing Mun Fung <yumeminami@users.noreply.github.com>
Homepage: https://github.com/yumeminami/rosbridge_server_rs
Depends: $depends, ros-jazzy-rcl, ros-jazzy-rcl-action, ros-jazzy-rmw-implementation, ros-jazzy-rosidl-runtime-c, ros-jazzy-rosapi-msgs, ros-jazzy-rcl-interfaces, ros-jazzy-rosgraph-msgs, ros-jazzy-rmw-fastrtps-cpp
Description: ROS 2 rosbridge WebSocket server written in Rust
 Implements the rosbridge WebSocket protocol and native rosapi services.
 Requires Ubuntu 24.04 and ROS 2 Jazzy. Source the ROS environment before use.
CONTROL
dpkg-deb --root-owner-group --build "$root" "$output/$name.deb"
tar -czf "$output/$name.tar.gz" \
    -C "$root/usr/bin" rosbridge_server_rs \
    -C "$PWD" LICENSE README.md
(cd "$output" && sha256sum "$name.deb" "$name.tar.gz" > "SHA256SUMS-$arch")
