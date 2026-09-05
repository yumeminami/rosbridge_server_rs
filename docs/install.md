# Installation

Version 0.1.0 targets Ubuntu 24.04 with ROS 2 Jazzy on amd64 and arm64.
Install ROS 2 Jazzy and configure its apt repository first, following the
[ROS installation guide](https://docs.ros.org/en/jazzy/Installation/Ubuntu-Install-Debs.html).
No Rust compiler or Python rosbridge server is needed.

## Debian package

Download the package matching `dpkg --print-architecture` from
[the release page](https://github.com/yumeminami/rosbridge_server_rs/releases/tag/v0.1.0).

```bash
sudo apt install ./rosbridge-server-rs_0.1.0_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

For ARM64, replace `amd64` with `arm64`. APT installs declared runtime dependencies
from your configured repositories. The package installs `/usr/bin/rosbridge_server_rs`;
it does not start a background service. Download and install a newer `.deb` to upgrade.
There is no project apt repository yet.

```bash
sudo apt remove rosbridge-server-rs
```

## Binary archive

Download the matching `.tar.gz` from the same release page. Archives contain the
binary, license and README; they do not bundle ROS or system libraries. On an
Ubuntu 24.04 host with Jazzy installed:

```bash
sudo apt install ros-jazzy-rosapi-msgs ros-jazzy-rcl ros-jazzy-rcl-action \
  ros-jazzy-rcl-interfaces ros-jazzy-rosgraph-msgs ros-jazzy-rmw-fastrtps-cpp
tar -xzf rosbridge-server-rs_0.1.0_ubuntu24.04_amd64.tar.gz
source /opt/ros/jazzy/setup.bash
./rosbridge_server_rs
```

Use the matching `arm64` archive on ARM64. The `.deb` is preferred because it
also declares the binary's exact shared-library package dependencies.

## Checksums

Download `SHA256SUMS` alongside your selected assets. From that directory:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

## Connect

Open a rosbridge WebSocket connection to `ws://localhost:9090`, or replace
`localhost` with the server's address. The default bind address is `0.0.0.0:9090`.
For local-only access, run `rosbridge_server_rs --bind 127.0.0.1:9090`.
Native rosapi is enabled by default. Use `--no-rosapi` when another node already
provides `/rosapi/*`. See [usage](usage.md) for options and limitations.

Custom message packages must be installed, including their C introspection and
typesupport libraries. Source their workspace after `/opt/ros/jazzy/setup.bash`.
Python package distribution through uv/uvx is not provided in this release.
