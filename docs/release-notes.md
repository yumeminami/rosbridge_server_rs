# rosbridge_server_rs 0.1.0

First early release of the Rust ROS 2 rosbridge WebSocket server.

- Topic, service and Action operations using the Python rosbridge wire protocol.
- All 29 native rosapi service interfaces, with no Python rosapi subprocess.
- Runtime message introspection, including custom message packages.
- JSON, CBOR, CBOR-RAW, PNG and fragmentation support.
- Ubuntu 24.04 / ROS 2 Jazzy packages for amd64 and arm64.

## Install

With ROS 2 Jazzy installed and its apt repository configured, download the `.deb`
for your architecture and run:

```bash
sudo apt install ./rosbridge-server-rs_0.1.0_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

Use the `arm64` filename on ARM64. Connect to `ws://localhost:9090`.
Source your workspace before starting if you use custom message packages.
Archives require the same external ROS runtime libraries. `SHA256SUMS` covers
all four packages.

## Scope

This early release does not establish long-running production stability or
complete compatibility with every Python rosbridge configuration. Use absolute
ROS names. TLS and authentication require a reverse proxy. Other ROS distributions
and native macOS/Windows are not supported by these packages.

CI gates release publication on formatting, protocol and ROS integration tests,
upstream WebSocket compatibility, and clean-runtime package smoke tests on both
architectures. See CHANGELOG.md and docs/usage.md for details.
