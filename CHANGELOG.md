# Changelog

Changes are recorded here before release. No version has been released yet.

## Unreleased

### Added

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

- Document the Ubuntu native build dependencies in the README.
- Separate topic, service and Action handling from protocol dispatch.
- Separate WebSocket connection handling from the command-line entry point.
- Use shared test scripts and a portable ROS workspace mount in Docker Compose.
- Consolidate licensing and third-party notices in a single `LICENSE` file.
