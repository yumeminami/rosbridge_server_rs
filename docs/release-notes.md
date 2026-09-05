# rosbridge_server_rs 0.1.4

Improves log filenames, terminal readability and service logging controls.

- Active files use YYYYMMDDHHmm.logging; rotation and graceful shutdown archive
  them as YYYYMMDDHHmm.log. Names and rotation follow the selected log timezone.
- Same-minute restarts add a numeric suffix instead of overwriting logs.
  Unfinished .logging files are preserved; max_files counts completed archives.
- Terminal level colors return, with plain field names and no bold styling.
  Redirected output and file logs remain uncolored.
- TOML and CLI log directories expand ~ and ~/... against HOME.
- Normal service calls and responses move to DEBUG. Timeouts remain WARN and
  failures remain ERROR. Payload previews remain separately opt-in.
- Humble / Ubuntu 22.04 and Jazzy / Ubuntu 24.04 packages for Linux x86_64 and ARM64.

## Configuration upgrade

First startup with v0.1.4 refreshes the managed
~/.rosbridge_server_rs/rosbridge.toml with the bundled defaults.
Same-version restarts preserve edits. To keep settings across upgrades, save
another file and use --config /path/to/custom.toml; explicit files are not modified.

Defaults are local time, terminal colors enabled, INFO level and no file output.
Use a dedicated log directory. Older rosbridge_server_rs.log.* files are left in
place and are not counted by the new timestamped archive retention.

```bash
rosbridge_server_rs --log-directory "$HOME/logs/rosbridge" --log-timezone local
```

Enable service call details without payloads:

```bash
rosbridge_server_rs --log-level 'info,rosbridge_server_rs::service_calls=debug'
```

To also inspect request/response contents, enable
rosbridge_server_rs::service_payload=debug. Previews are capped at 4096 UTF-8
bytes, marked when truncated, and are not redacted. INFO logs contain neither
normal service lifecycle entries nor payloads.

## Install

With Humble or Jazzy installed, download the matching `.deb` and run:

```bash
sudo apt install ./rosbridge-server-rs_0.1.4_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

Use `humble_ubuntu22.04` for Humble and `arm64` on ARM64. Connect to
`ws://localhost:9090`.
Source your workspace before starting if you use custom message packages.
Archives require the same external ROS runtime libraries. `SHA256SUMS` covers
all release packages.

Install from PyPI with
`uv tool install rosbridge_server_rs==0.1.4`, then run `rosbridge_server_rs`, or use
`uvx rosbridge_server_rs==0.1.4` directly. Source the ROS environment first. uv selects
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
