# rosbridge_server_rs 0.1.5

Improves slow-client handling, scheduling fairness and WebSocket liveness, and
moves outbound protocol encoding off the shared ROS worker.

- Topic congestion drops whole batches while keeping the connection and
  subscriptions. Positive queue_length enables oldest-first replacement per topic,
  including with throttle_rate=0.
- Service/action/status messages have reserved capacity. Control overload and
  encoding failures close with explicit reasons rather than silently losing replies.
- Per-client incoming queues are served round-robin; disconnect and shutdown
  notifications do not compete for data capacity.
- Active ping/pong probes detect unresponsive clients. Socket reads and writes
  progress concurrently, with bounded writes and closing handshakes.
- JSON/CBOR/PNG encoding and fragmentation run in bounded blocking jobs. Waiting
  requests stay in bounded queues and can be discarded before encoding.

## Configuration upgrade

The managed ~/.rosbridge_server_rs/rosbridge.toml is refreshed on first startup
with a new version. Preserve custom settings in a separate --config file.

```toml
incoming_queue_size = 256       # Now per connection, previously server-wide.
write_queue_size = 64
write_queue_bytes = 67108864    # Estimated queued input per lane; also caps output batch.
encoding_workers = 2
websocket_ping_interval = 30.0  # Set 0 to disable active probes.
websocket_ping_timeout = 30.0
```

Total input capacity grows with the number of connections. Queue byte estimates
are not an RSS limit: codec temporaries, subscription buffers and in-flight
batches remain additional. Encoding concurrency is shared across connections;
ROS message conversion remains on the ROS worker, and no cross-client encoding
cache is introduced. Performance gains depend on the workload and are not yet
quantified on real robots.

## Validation

61 local non-ROS tests passed, including queue saturation, ordering, cancellation,
encoding compatibility and real local WebSocket heartbeat/close tests. Clippy
passed with Humble doc-only bindings. The tag pipeline gates publication on native
Humble/Jazzy checks, upstream compatibility and package/wheel smoke tests for both
x86_64 and ARM64.

## Install

With ROS 2 Jazzy installed:

```bash
sudo apt install ./rosbridge-server-rs_0.1.5_jazzy_ubuntu24.04_amd64.deb
source /opt/ros/jazzy/setup.bash
rosbridge_server_rs
```

Use humble_ubuntu22.04 for Humble and arm64 on ARM64. Source custom message
workspaces before starting. Archives require the same external ROS libraries.
SHA256SUMS covers the release packages.

```bash
uv tool install rosbridge_server_rs==0.1.5
# Or: uvx rosbridge_server_rs==0.1.5
```

For an existing uv tool, use `uv tool upgrade rosbridge_server_rs`.
This is an early prerelease; it does not establish long-running production
stability or complete parity with every Python rosbridge configuration.
