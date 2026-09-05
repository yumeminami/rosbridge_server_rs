# Compatibility tests and WebSocket benchmark

Run from the repository root. Set `ROSBRIDGE_SUITE_PATH` to the local rosbridge_suite checkout before invoking Compose; it is mounted read-only at `/upstream`.

```bash
export ROSBRIDGE_SUITE_PATH=/path/to/rosbridge_suite
docker compose build test
docker compose run --rm test bash benchmarks/run.sh
```

The runner builds upstream `rosbridge_test_msgs`, runs Rust tests, builds the release server, runs the upstream WebSocket cases against both servers, and measures matched topic roundtrips. Results and logs are written under `benchmarks/results`; the plot is saved as PNG and SVG.

## Test scope

`src/ros/message_tests.rs` ports the cases in upstream `rosbridge_library/test/internal/test_message_conversion.py`: signed/unsigned integer ranges and overflow, byte/char/bool/string messages, Time and Duration, Header and Header arrays, assorted default messages, byte arrays with list/Base64 input, float arrays, bounded/nested arrays, and non-finite output. Rust tests exercise native introspection rather than Python private methods. Existing protocol tests cover publisher ownership, subscription coalescing, QoS, service correlation, timeout and client isolation, fragmentation, PNG, and CBOR.

`parity.py` imports each upstream `rosbridge_server/test/websocket/*.test.py` unchanged and executes its unittest assertions. The adapter supplies a known WebSocket port because Rust does not expose Python's `actual_port` ROS parameter. The starvation publisher is launched explicitly in place of its launch description. Each case runs in a fresh process because the Twisted reactor is not restartable.

This is not a port of every Python-internal manager, rosapi, or stress test. `results/parity.json` records individual outcomes, including failures and timeouts. Logs retain assertion details.

## Compatibility results

The Rust checks cover 15 native message-conversion tests, 19 protocol tests, and 16 ROS/WebSocket integration tests. All 13 unchanged upstream WebSocket cases pass on both Python and Rust; individual outcomes are recorded in `results/parity.json`.

Service request IDs now match Python's `service_request:<service>:<counter>` format, with a counter scoped to the service advertisement. Regression tests also verify encoding precedence across shared subscriptions and Python's handling of CBOR with `fragment_size`.

The tests exposed and verified fixes for Time aliases, Header arrays, Action type spelling, service timeout text, and retained transient-local samples for late WebSocket clients.

## Benchmark method

- Python baseline: installed ROS 2 Jazzy rosbridge server 2.7.0. The local 4.2.1 checkout requires `rosidl_pycommon.interface_base_classes`, unavailable in Jazzy, and is not the measured server.
- Rust: optimized `cargo build --release --locked`.
- Same container, localhost, ROS domain, JSON encoding, no WebSocket compression, reliable/volatile QoS with depth 256.
- One connection advertises and subscribes to a topic. Each message travels through the server's native ROS publisher and subscription before returning over WebSocket.
- Payloads: 64 B, 1 KiB, 64 KiB strings, including a 16-digit sequence number.
- Windows: one and 32 messages in flight. Twenty roundtrips warm up each trial; measurement lasts two seconds plus pending-message drain. Three independent process runs per configuration, alternating server order.
- The client verifies every sequence number and payload. Timeout, duplicate, or missing responses fail the run.
- Latency includes client JSON work, WebSocket transport, conversion, ROS transport, and server scheduling. It is not a serialization microbenchmark or a maximum-capacity claim.
- CPU measures server process CPU time divided by elapsed time; 100% equals one core. RSS is the larger of the before/after samples, not a continuously sampled peak. Client resource usage is excluded.
- Plot bars show means of trial summaries; whiskers show min/max across three trials, not confidence intervals. Trials are short and Docker/macOS scheduling affects results.

`raw.json` retains per-message latencies and trial summaries. `environment.json` records versions and configuration. Run `python3 benchmarks/plot.py` inside the test container to regenerate the plots.
