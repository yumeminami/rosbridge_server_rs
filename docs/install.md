# Installation

Version 0.1.1 targets ROS 2 Humble on Ubuntu 22.04 and ROS 2 Jazzy on Ubuntu
24.04, on amd64 and arm64. Install ROS and configure its apt repository first,
following the [Humble](https://docs.ros.org/en/humble/Installation.html) or
[Jazzy](https://docs.ros.org/en/jazzy/Installation.html) installation guide.
No Rust compiler or Python rosbridge server is needed.

## Debian package

Download the package matching `dpkg --print-architecture` from
[the release page](https://github.com/yumeminami/rosbridge_server_rs/releases/tag/v0.1.1).

```bash
sudo apt install ./rosbridge-server-rs_0.1.1_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

For Humble use `humble_ubuntu22.04`; for ARM64 use `arm64`. APT installs dependencies
from your configured repositories. The package installs `/usr/bin/rosbridge_server_rs`;
`rosbridge-server-rs` is an equivalent command. The package does not start a
background service. Download and install a newer `.deb` to upgrade.
There is no project apt repository yet.

```bash
sudo apt remove rosbridge-server-rs
```

## Binary archive

Download the matching `.tar.gz` from the same release page. Archives contain the
binary, license and README; they do not bundle ROS or system libraries. On a
supported host with ROS installed:

```bash
sudo apt install ros-$ROS_DISTRO-rosapi-msgs
tar -xzf rosbridge-server-rs_0.1.1_jazzy_ubuntu24.04_amd64.tar.gz
source /opt/ros/$ROS_DISTRO/setup.bash
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
typesupport libraries. Source their workspace after the ROS setup file.

## uv and uvx

Install and run from PyPI:

```bash
source /opt/ros/jazzy/setup.bash # Use humble for ROS 2 Humble.
uv tool install rosbridge_server_rs
rosbridge_server_rs
```

Or run directly:

```bash
uvx rosbridge_server_rs
```

uv chooses the wheel for x86_64 or ARM64; the launcher selects its Humble or
Jazzy binary from `$ROS_DISTRO`. Both command names, `rosbridge_server_rs` and
`rosbridge-server-rs`, are installed. ROS and `ros-$ROS_DISTRO-rosapi-msgs`
must already be installed; uv does not install system packages.

For offline installation, download the release wheels into `dist` and run `uvx --no-index --find-links ./dist rosbridge_server_rs`.
