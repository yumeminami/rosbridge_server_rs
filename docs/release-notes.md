# rosbridge_server_rs 0.1.1

Adds uv/uvx distribution and ROS 2 Humble support to the Rust rosbridge server.

- Topic, service and Action operations using the Python rosbridge wire protocol.
- All 29 native rosapi service interfaces, with no Python rosapi subprocess.
- Runtime message introspection, including custom message packages.
- JSON, CBOR, CBOR-RAW, PNG and fragmentation support.
- Humble / Ubuntu 22.04 and Jazzy / Ubuntu 24.04 packages for amd64 and arm64.
- Platform wheels that select the architecture and `$ROS_DISTRO` at runtime.

## Install

With Humble or Jazzy installed, download the matching `.deb` and run:

```bash
sudo apt install ./rosbridge-server-rs_0.1.1_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

Use `humble_ubuntu22.04` for Humble and `arm64` on ARM64. Connect to
`ws://localhost:9090`.
Source your workspace before starting if you use custom message packages.
Archives require the same external ROS runtime libraries. `SHA256SUMS` covers
all release packages.

Once the wheels are published to PyPI, install with
`uv tool install rosbridge_server_rs`, then run `rosbridge_server_rs`, or use
`uvx rosbridge_server_rs` directly. Source the ROS environment first. uv selects
the architecture, and the launcher selects the binary using `$ROS_DISTRO`.

## Scope

This early release does not establish long-running production stability or
complete compatibility with every Python rosbridge configuration. Use absolute
ROS names. TLS and authentication require a reverse proxy. ROS distributions other
than Humble and Jazzy, and native macOS/Windows, are not supported by these packages.

CI gates release publication on formatting, protocol and ROS integration tests,
upstream WebSocket compatibility, and package smoke tests for both
distributions and architectures. See CHANGELOG.md and docs/usage.md for details.
