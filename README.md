# rosbridge_server_rs

[![CI](https://github.com/yumeminami/rosbridge_server_rs/actions/workflows/ci.yml/badge.svg)](https://github.com/yumeminami/rosbridge_server_rs/actions/workflows/ci.yml)
[![License: EPL-2.0 OR Apache-2.0](https://img.shields.io/badge/License-EPL--2.0%20OR%20Apache--2.0-blue)](LICENSE)
[![ROS 2: Humble | Jazzy](https://img.shields.io/badge/ROS%202-Humble%20%7C%20Jazzy-blue)](docs/usage.md)

**A fast, lightweight ROS 2 WebSocket server, written in Rust.**

Use the Python rosbridge protocol to publish and subscribe to topics, call services,
and exchange Action goals. The server connects through native RCL/RMW and loads
message definitions at runtime, including custom interfaces. No generated Rust
message bindings or Python server process are required.

## Install and run

Requires **ROS 2 Humble on Ubuntu 22.04** or **ROS 2 Jazzy on Ubuntu 24.04**.
Download the `.deb` matching the ROS distribution and architecture from
[v0.1.1](https://github.com/yumeminami/rosbridge_server_rs/releases/tag/v0.1.1):
`amd64` for Intel/AMD, or `arm64` for ARM64. With the ROS apt repository configured:

```bash
sudo apt install ./rosbridge-server-rs_0.1.1_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

`rosbridge-server-rs` is provided as an equivalent command.

With [uv](https://docs.astral.sh/uv/), download both release wheels into one
directory. uv selects x86_64 or ARM64, and the launcher reads `$ROS_DISTRO` to
select Humble or Jazzy:

```bash
source /opt/ros/jazzy/setup.bash
uvx --no-index --find-links ./dist rosbridge-server-rs
```

Use the `arm64` filename on ARM64. Connect your client to `ws://localhost:9090`
(or the server's IP address). All 29 rosapi services are built in.
Source your ROS workspace before starting the server if you use custom messages.

Wheel download, persistent uv installation, archives and checksums:
[installation guide](docs/install.md).
Options: [usage](docs/usage.md). Developers: [build from source](docs/building.md).

## Protocol support

| Area | Supported behavior |
| --- | --- |
| Topics | Publish, subscribe, advertise, QoS, throttling and shared subscriptions |
| Services | Calls and advertisements in both directions, request IDs and timeouts |
| Actions | Goals, feedback, results and cancellation in both directions |
| Messages | Nested fields, arrays, bounded sequences, defaults and Base64 bytes |
| Encoding | JSON, CBOR, CBOR-RAW, PNG and JSON fragmentation |

**All 13 upstream WebSocket cases passed on both Rust and Python in the recorded Jazzy run.** Additional tests
cover message conversion, resource ownership and error handling. See the
[compatibility results](benchmarks/README.md#compatibility-results).

This is an early implementation and does not support all Python server
configuration. Use absolute ROS names. Native macOS builds,
other ROS distributions and production stability remain unverified.
[Protocol boundaries](docs/usage.md#limitations) are documented explicitly.

## Benchmarks

Measured against Python rosbridge on ROS 2 Jazzy. Throughput, live subscriptions
and idle behavior are separate experiments; their results should be read independently.

### Topic throughput

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="benchmarks/results/comparison-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="benchmarks/results/comparison.svg">
  <img alt="WebSocket throughput: Rust delivers 37,820, 27,761 and 2,588 messages per second for 64 B, 1 KiB and 64 KiB payloads; Python rosbridge 2.7.0 delivers 2,572, 2,435 and 1,336." src="benchmarks/results/comparison.svg" width="1000">
</picture>

JSON topic roundtrips on Linux ARM64, with 32 messages in flight: **37,820 vs
2,572 messages/s** for a 64-byte payload. Rust release build versus Python
rosbridge 2.7.0; three 2-second trials per configuration on localhost.
[Method and reproduction](benchmarks/README.md) · [Full results](benchmarks/results/README.md)

### Live subscriptions

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="benchmarks/results/live/comparison-dark.svg">
  <img alt="Live diagnostics subscriptions: mean container CPU was 3.64% for Rust and 19.80% for Python plus rosapi across 30 samples." src="benchmarks/results/live/comparison-light.svg" width="1000">
</picture>

Two simultaneous browser sessions subscribed to `/diagnostics` on the same Linux
x86_64 host. Both measurements include rosapi.

| Mean resource use | Rust | Python |
| --- | ---: | ---: |
| Container CPU | **3.64%** | 19.80% |
| Service process RSS | **29.5 MiB** | 168.0 MiB |

CPU is relative to one core, sampled 30 times at approximately two-second intervals.
RSS was collected in a separate 30-sample observation: the Rust process versus the
sum of Python WebSocket and rosapi processes; shared pages may be counted twice.
[Samples and measurement limits](benchmarks/results/live/README.md)

### Idle behavior

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="benchmarks/results/idle/comparison-dark.svg">
  <img alt="Idle CPU and memory on Linux x86_64. All three trials are shown for Python rosbridge plus rosapi and Rust with native rosapi." src="benchmarks/results/idle/comparison-light.svg" width="1000">
</picture>

After unsubscribe, median CPU was **0.40% for Rust vs 0.80% for Python**.
With a connection and no subscriptions, both were approximately **0.50%**.
Linux x86_64, including rosapi; three 10-second trials per state.
[Samples and reproduction](benchmarks/results/idle/README.md)

These measurements describe the tested workloads, not production capacity or
camera-stream performance.

## Development

Protocol tests run without ROS:

```bash
cargo test --locked --no-default-features
cargo fmt --check
```

For native tests, place `rosbridge_suite` beside this repository or set
`ROSBRIDGE_SUITE_PATH` to its location, then run:

```bash
scripts/ci_container.sh jazzy
# Or: scripts/ci_container.sh humble
```

The script pulls an official ROS base image and reuses one container plus Rust
volumes across runs. It installs dependencies once, then runs conversion,
protocol, WebSocket and package tests. See [benchmarks](benchmarks/README.md) for
the separate compatibility and performance runners.

```text
src/
  main.rs          Command-line entry point
  server.rs        WebSocket connections and ROS worker
  bridge/          Protocol state, topics, services and actions
  backend.rs       ROS backend contract and QoS
  ros/             Native handles and runtime message conversion
  wire.rs          Encoding and fragmentation
tests/             Protocol and ROS/WebSocket integration tests
scripts/           Test environment preparation and execution
benchmarks/        Reproducible measurements and plots
```

Use `cargo fmt` for Rust and `black --line-length 100` for Python. Keep native ROS
handles on the worker thread; WebSocket tasks communicate through bounded queues.

CI checks formatting, Clippy, protocol tests and upstream WebSocket compatibility
on native Linux x86_64 and ARM64 runners for Humble and Jazzy. It installs and
tests each package in its matching ROS container. Version tags publish
tested `.deb`, `.tar.gz` and wheel packages with SHA-256 checksums.

See [CHANGELOG](CHANGELOG.md) for release history.

## License

EPL-2.0 OR Apache-2.0. See [LICENSE](LICENSE), including third-party notices.
