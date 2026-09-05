# rosbridge_server_rs 0.1.2

Adds automatic TOML configuration, forwarding allowlists and rotating file logs.

- Creates and loads ~/.rosbridge_server_rs/rosbridge.toml on first startup,
  including through uvx and uv-installed commands. Existing files are preserved.
- Enforces topics_glob, topics_pub_glob, topics_sub_glob, services_glob and
  params_glob on forwarding, with action transport checks and parameter-service
  bypass protection. Omitted lists allow all; empty lists deny all.
- Adds configurable file logging with daily/hourly rotation and retention.
- Logs client connection counts, handshake metadata, session duration,
  subscriptions and failed operations.
- Includes a measured ARM64 SoC CPU/RSS comparison and visible README charts.
- Continues to support Humble / Ubuntu 22.04 and Jazzy / Ubuntu 24.04,
  on Linux x86_64 and ARM64, with architecture-selecting uv wheels.

Configuration and forwarding changes are documented in docs/configuration.md.
Topic/service globs now enforce access, rather than only filtering discovery.
The common topics_glob list is added to both directional topic lists.
When params_glob is set, use rosapi parameter methods instead of raw parameter
services. Allow needed rosapi methods explicitly in services_glob.

## Install

With Humble or Jazzy installed, download the matching `.deb` and run:

```bash
sudo apt install ./rosbridge-server-rs_0.1.2_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

Use `humble_ubuntu22.04` for Humble and `arm64` on ARM64. Connect to
`ws://localhost:9090`.
Source your workspace before starting if you use custom message packages.
Archives require the same external ROS runtime libraries. `SHA256SUMS` covers
all release packages.

Install from PyPI with
`uv tool install rosbridge_server_rs==0.1.2`, then run `rosbridge_server_rs`, or use
`uvx rosbridge_server_rs==0.1.2` directly. Source the ROS environment first. uv selects
the architecture, and the launcher selects the binary using `$ROS_DISTRO`.

To upgrade an existing uv tool, run `uv tool upgrade rosbridge_server_rs`.
The default configuration is created when the server starts, not during wheel
installation. Edit it and restart; explicit CLI flags override the file.

## Scope

This early release does not establish long-running production stability or
complete compatibility with every Python rosbridge configuration. Use absolute
ROS names. TLS and authentication require a reverse proxy. ROS distributions other
than Humble and Jazzy, and native macOS/Windows, are not supported by these packages.

CI gates release publication on formatting, protocol and ROS integration tests,
upstream WebSocket compatibility, and package smoke tests for both
distributions and architectures. See CHANGELOG.md and docs/usage.md for details.
