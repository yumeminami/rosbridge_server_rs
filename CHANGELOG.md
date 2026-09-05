# Changelog

Release history for rosbridge_server_rs.

## 0.1.3 — 2026-09-06

- Refresh the managed default TOML on version changes; preserve explicit --config files and same-version edits.

- Add opt-in DEBUG service payload previews, capped at 4096 UTF-8 bytes.

- Log service calls, responses and timeouts with client/request correlation and elapsed time.

- Add CLI overrides for log directory, filter, timezone and ANSI styling.
- Use plain console logs and local timestamps with explicit timezone offsets.

## 0.1.2 — 2026-09-06

- Create and load ~/.rosbridge_server_rs/rosbridge.toml on first server startup, preserving existing files.
- Enforce topic, service and parameter allowlists on WebSocket forwarding as well as rosapi discovery; check action transports and block raw parameter-service bypasses.

- Add explicit TOML configuration with CLI precedence and documented Python launch parameter mapping.
- Add rotating file logs with tracing-appender and client handshake/session metadata.

- Log client connections, active client counts, topic subscriptions and cleanup
  at INFO, and failed protocol operations with their resource name at ERROR.
- Add the 61-second ARM64 SoC CPU/RSS observation, raw samples and README charts.

## 0.1.1 — 2026-09-06

### Added

- Platform wheels for uv and uvx, with automatic x86_64 and ARM64 selection.
- Clean-runtime uvx and persistent uv tool installation checks on both architectures.
- ROS 2 Humble packages and integration tests alongside Jazzy.
- Record Humble Python rosbridge's event-loop starvation failure separately;
  the Rust implementation must still pass the same test.
- Accept Cyclone DDS or Fast DDS as the installed RMW and provide both underscore
  and hyphen command names.

## 0.1.0 — 2026-09-05

First early release for Ubuntu 24.04 and ROS 2 Jazzy, on amd64 and arm64.

### Added

- Installable Debian packages and binary archives published to GitHub Releases.
- Clean-runtime installation, native rosapi and ROS topic delivery smoke tests.

- Implement all 29 rosapi service interfaces natively in Rust, including graph
  queries, recursive type definitions and asynchronous parameter operations.
- Compare rosapi interfaces against Python and measure same-host idle CPU.
- Tag-triggered CI with native Linux x86_64 and ARM64 builds and ROS 2 tests.
- Release binary archives and SHA-256 checksums for tag and manual workflow runs.
- Rust ROS 2 WebSocket server with rosbridge topic, service and Action operations.
- Runtime message introspection for custom interfaces, nested messages and arrays.
- JSON, CBOR, CBOR-RAW, PNG and JSON fragmentation support.
- Subscription QoS, throttling, shared entities and connection-owned cleanup.
- Service response deadlines and Action acceptance timeouts.
- Native message tests, protocol tests and ROS 2 WebSocket integration tests.
- An adapter that runs the upstream Python WebSocket cases against both servers.
- Reproducible throughput, latency, CPU and memory measurements, with README plots.
- GitHub Actions checks for formatting, Clippy, protocol tests and ROS 2 Jazzy
  integration. Upstream test sources are pinned; test logs and JUnit results are
  retained as artifacts.

### Fixed

- Accept legacy `rosapi` service type names from WebSocket clients as `rosapi_msgs`.
- Include requested and discovered types in interface mismatch errors.
- Validate Action results before caching them, allowing invalid results to be corrected.
- Release consumed service requests and isolate request errors from the ROS worker.
- Preserve queued topic-message order and replay transient-local samples to late clients.
- Accept ROS 1 Time field aliases and handle Header arrays as arrays.
- Match Python service request IDs, Action type spelling and service timeout messages.
- Match Python encoding precedence for shared subscriptions and ignore `fragment_size`
  for binary CBOR output.
- Return a failing exit status from the compatibility runner when a case fails,
  times out or cannot be found.

### Changed

- Wait on RCL events and wake the ROS worker with a guard condition for WebSocket
  commands; stop allocating and taking messages from inactive ROS entities.
- Remove the Python rosapi child process and its extra DDS participant.
- Document the Ubuntu native build dependencies in the README.
- Separate topic, service and Action handling from protocol dispatch.
- Separate WebSocket connection handling from the command-line entry point.
- Use shared test scripts and a portable ROS workspace mount in Docker Compose.
- Consolidate licensing and third-party notices in a single `LICENSE` file.
