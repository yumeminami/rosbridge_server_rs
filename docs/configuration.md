# Configuration and file logging

Available in source builds after v0.1.1. Start with the repository's
[`rosbridge.toml`](../rosbridge.toml):

```bash
rosbridge_server_rs --config rosbridge.toml
rosbridge_server_rs --config rosbridge.toml --bind 0.0.0.0:8443
```

Precedence is explicit CLI flags, TOML settings, then existing Rust defaults.
`RUST_LOG` overrides `log.level`. Unknown TOML keys and invalid values stop startup.
No file is discovered implicitly. Relative paths use the process working directory.
ROS arguments after `--` remain supported and override TOML-generated rosapi parameters.

## File logging

```toml
[log]
level = "info"
console = true
directory = "/var/log/rosbridge"
rotation = "daily"
max_files = 7
```

`tracing-appender` writes files through a bounded background queue. Daily and hourly
rotation use UTC; `never` appends to one file. `max_files` retains matching rotated
files; it is not a size limit. The writer flushes on graceful shutdown. Its default
lossy queue can drop log lines if disk writes cannot keep up. Console output goes
to stderr; file output has no ANSI color codes. Ensure the configured directory
is writable. In Docker, use a bind-mounted directory to preserve logs.

Handshake logs include a numeric connection ID, socket peer, URL path, Origin,
User-Agent and `forwarded_for`. Forwarding headers are client-supplied metadata,
not verified identity; behind Caddy the socket peer is the proxy. Session-end logs
include duration and the received close code/reason when available. Use the
connection ID to correlate these with subscription and error logs. No query
strings, cookies, Authorization headers or message payloads are recorded.

## Python launch parameter mapping

Compared with `rosbridge_server/launch/rosbridge_websocket_launch.xml` in the
local upstream checkout. This is partial launch-configuration compatibility,
not a claim that every Python execution setting has identical semantics.

| Python launch parameter | TOML support / behavior |
| --- | --- |
| `address`, `port` | Supported; empty address means `0.0.0.0`. IP literals only. CLI `--bind` overrides both. |
| `url_path` | Supported; exact path matching, default `/`. |
| `namespace` | Supported; also `--namespace`. |
| `max_message_size` | Supported; retains Rust default 16 MiB, versus Python's 10,000,000 bytes. |
| `incoming_queue_size` | Supported as the global bounded ROS command queue, default 256; not a separate queue per client. |
| `write_queue_size` | Supported as per-client outbound batches, default 64; a batch may contain multiple frames. Slow clients are disconnected when full. |
| `default_call_service_timeout` | Supported via `service_timeout`; retains Rust default 30 seconds. Positive values only. |
| `fragment_timeout` | Supported; retains Rust default 30 seconds. Expired assemblies are removed on the next received frame. |
| `topics_glob`, `topics_pub_glob`, `topics_sub_glob` | Arrays of glob strings; filter native rosapi discovery. They do **not** restrict WebSocket publish/subscribe access. |
| `services_glob` | Filters native rosapi service discovery; not a WebSocket service-call access rule. |
| `params_glob`, `params_timeout` | Supported by native rosapi; timeout defaults to 5 seconds. |
| `ssl`, `certfile`, `keyfile` | Not accepted; terminate TLS in a reverse proxy. |
| `use_compression` | Not accepted; WebSocket permessage-deflate is not implemented. Protocol CBOR/PNG remains supported. |
| `websocket_ping_interval`, `websocket_ping_timeout` | Not accepted; incoming ping frames receive pong, but server-initiated heartbeat scheduling is not implemented. |
| `delay_between_messages` | Not accepted; no artificial inter-message delay. |
| `unregister_timeout` | Not accepted; cleanup is immediate. |
| `retry_startup_delay` | Not accepted; bind failures return an error. |
| `call_services_in_new_thread`, `send_action_goals_in_new_thread`, `use_events_executor` | Not accepted; Rust uses its own ROS worker and Tokio WebSocket tasks. |
| `respawn` | Launch/supervisor responsibility; use systemd or Docker restart policy. |

Rust-specific TOML keys include `node_name`, `use_sim_time`, `no_rosapi`, and the
`[log]` table. Omit rosapi glob settings to keep existing discovery behavior.
