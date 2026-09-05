# Configuration and file logging

Available since v0.1.2. Start with the repository's
[`rosbridge.toml`](../rosbridge.toml):

```bash
rosbridge_server_rs --config rosbridge.toml
rosbridge_server_rs --config rosbridge.toml --bind 0.0.0.0:8443
```

Precedence is explicit CLI flags, TOML settings, then existing Rust defaults.
`RUST_LOG` overrides `log.level`. Unknown TOML keys and invalid values stop startup.
Without `--config`, the server creates and reads
`~/.rosbridge_server_rs/rosbridge.toml`. Existing files are never overwritten;
`--config` selects only the specified file and does not create the default.
This happens on first server startup, including through `uvx` or a uv-installed
command. Wheel installation has no post-install hook, so `uv tool install`
alone cannot create a file in the user's home. `--help` and `--version` do not
create files. Relative paths use the process working directory.
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
| `topics_glob`, `topics_pub_glob`, `topics_sub_glob` | Forwarding allowlists for advertisements, publications and subscriptions, also used by native rosapi discovery. |
| `services_glob` | Allowlist for WebSocket service calls and advertisements, also used by native rosapi discovery. |
| `params_glob`, `params_timeout` | Parameter-name allowlist for native rosapi and WebSocket parameter calls; timeout defaults to 5 seconds. |
| `ssl`, `certfile`, `keyfile` | Not accepted; terminate TLS in a reverse proxy. |
| `use_compression` | Not accepted; WebSocket permessage-deflate is not implemented. Protocol CBOR/PNG remains supported. |
| `websocket_ping_interval`, `websocket_ping_timeout` | Not accepted; incoming ping frames receive pong, but server-initiated heartbeat scheduling is not implemented. |
| `delay_between_messages` | Not accepted; no artificial inter-message delay. |
| `unregister_timeout` | Not accepted; cleanup is immediate. |
| `retry_startup_delay` | Not accepted; bind failures return an error. |
| `call_services_in_new_thread`, `send_action_goals_in_new_thread`, `use_events_executor` | Not accepted; Rust uses its own ROS worker and Tokio WebSocket tasks. |
| `respawn` | Launch/supervisor responsibility; use systemd or Docker restart policy. |

Rust-specific TOML keys include `node_name`, `use_sim_time`, `no_rosapi`, and the
`[log]` table. Omit allowlists to keep unrestricted forwarding and discovery.

## Forwarding allowlists

These rules apply to all WebSocket clients. Denied operations return a protocol
error and are logged before a ROS entity or request is created. Restart the server
after editing configuration.

| Setting | Meaning |
| --- | --- |
| `topics_glob` | Common topic allowlist, added to both directional lists (Python-compatible union). |
| `topics_pub_glob` | Topics the WebSocket client may advertise or publish into ROS, including implicit publication. |
| `topics_sub_glob` | Topics the WebSocket client may subscribe to and receive from ROS. |
| `services_glob` | Services the WebSocket client may call or advertise. No implicit exemption for `/rosapi/*`. |
| `params_glob` | Short parameter names, e.g. `use_sim_time` or `camera.*`, consistently applied to reads, writes, deletion, existence checks and returned name lists. |

Omission means unrestricted; `[]` means deny all. Patterns are case-sensitive
and support `*`, `?` and character classes; `*` can span slashes. Topic and
service names are matched with a leading slash, before server-configured ROS
remapping. Use quoted strings in TOML. Explicit ROS `-p` values override file
values for the same key.

For asymmetric access, leave `topics_glob = []` and set the directional lists.
A permissive common pattern such as `topics_glob = ["*"]` also permits both
directions even when a directional list is empty.

Action operations must pass the service allowlist for
`<action>/_action/{send_goal,get_result,cancel_goal}` and the topic allowlist for
`<action>/_action/{feedback,status}`: subscription permission for action clients,
publication permission for client-advertised action servers.

When `params_glob` is configured, raw rcl_interfaces parameter-service calls and
client-advertised parameter services (including rosapi parameter methods) are rejected to prevent bypassing
parameter filtering. Use the rosapi parameter methods and allow their service names
in `services_glob`. Returned parameter names retain the `/node:parameter` format,
but filtering consistently uses the short parameter name.

The README's read-only example permits only six discovery/time services.
Add other discovery services explicitly if your viewer requests them. Native rosapi
uses these lists to filter discovery; the forwarding rules still apply with
`no_rosapi = true`. These are bridge forwarding permissions, not DDS permissions
for other ROS nodes or controls over the side effects of an allowed service.
