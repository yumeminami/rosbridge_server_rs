# rosbridge_server_rs 0.1.3

Adds local-time logging, CLI log controls and bounded service payload debugging.

- Plain console logs by default, with local timestamps and explicit timezone offsets.
- CLI overrides for log directory, filter, local/UTC timestamps and ANSI styling.
- Service call, response and timeout logs with client/request IDs and elapsed time.
- Opt-in DEBUG request/response previews, limited to 4096 UTF-8 bytes per entry.
- Anonymized benchmark topic names and SoC details moved off the README homepage.
- Humble / Ubuntu 22.04 and Jazzy / Ubuntu 24.04 packages for Linux x86_64 and ARM64.

## Configuration upgrade

**The default ~/.rosbridge_server_rs/rosbridge.toml is now a managed file.**
First startup with a different version replaces it with the bundled defaults.
Upgrading from v0.1.2 also replaces it because no version marker exists yet.
Same-version restarts preserve edits.

To retain your settings, copy them to another path before starting v0.1.3 and use:

```bash
rosbridge_server_rs --config /path/to/custom.toml
```

Explicit configuration files are never modified. New log defaults are local time,
ANSI styling disabled, INFO level and no file output. File rotation still uses UTC.
Enable service payload previews only when needed:

```bash
rosbridge_server_rs --log-level 'info,rosbridge_server_rs::service_payload=debug'
```

Previews are not redacted. INFO logs do not include payloads.

## Install

With Humble or Jazzy installed, download the matching `.deb` and run:

```bash
sudo apt install ./rosbridge-server-rs_0.1.3_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

Use `humble_ubuntu22.04` for Humble and `arm64` on ARM64. Connect to
`ws://localhost:9090`.
Source your workspace before starting if you use custom message packages.
Archives require the same external ROS runtime libraries. `SHA256SUMS` covers
all release packages.

Install from PyPI with
`uv tool install rosbridge_server_rs==0.1.3`, then run `rosbridge_server_rs`, or use
`uvx rosbridge_server_rs==0.1.3` directly. Source the ROS environment first. uv selects
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
