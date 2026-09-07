# Configuration and file logging

Available since v0.1.2. Start with the repository's
[`rosbridge.toml`](../rosbridge.toml):

```bash
rosbridge_server_rs --config rosbridge.toml
rosbridge_server_rs --config rosbridge.toml --bind 0.0.0.0:8443
```

Precedence is explicit CLI flags, TOML settings, then existing Rust defaults.
`--log-level` overrides `RUST_LOG`, which overrides TOML `log.level`. Unknown TOML keys and invalid values stop startup.
Without `--config`, the server creates and reads
`~/.rosbridge_server_rs/rosbridge.toml`. Since v0.1.3 this is a managed default:
on first startup with a different version it is overwritten with the bundled
configuration. A missing version marker (including upgrades from v0.1.2) also
refreshes it. Same-version restarts preserve edits; deleting the file recreates it.
The sibling `.config-version` file records the last version.

Use a separate file with `--config /path/to/custom.toml` for persistent settings.
Explicit configuration files are read without modification and skip default-file
refresh entirely.
This happens on first server startup, including through `uvx` or a uv-installed
command. Wheel installation has no post-install hook, so `uv tool install`
alone cannot create a file in the user's home. `--help` and `--version` do not
create files. Relative paths use the process working directory.
ROS arguments after `--` remain supported and override TOML-generated rosapi parameters.

## File logging

By default, logs go only to stderr; no log file is created. Set `log.directory`
to enable file output. Use an absolute path such as
`/home/xr/.rosbridge_server_rs/logs`. Version 0.1.4 and later also expand
`~` and `~/...` against HOME in both TOML and `--log-directory`. Version 0.1.3
treats a quoted tilde literally; use an absolute path or shell-expanded
`--log-directory "$HOME/logs"` with that version.

Version 0.1.3 defaults to plain console output. Version 0.1.4 and later
restore level colors on terminals while keeping field names unstyled. Redirected
output and log files stay plain. Timestamps use the process's local timezone with
an explicit UTC offset. In containers, configure the container timezone (for example
`TZ=Asia/Shanghai` with timezone data installed); the host's timezone may differ.
Version 0.1.2 uses UTC timestamps and styled console fields.

Version 0.1.3 and later also accept CLI overrides:

```bash
rosbridge_server_rs --log-directory /home/xr/.rosbridge_server_rs/logs \
  --log-level info --log-timezone local --log-ansi false
```

| CLI flag | TOML key | Default |
| --- | --- | --- |
| `--log-directory PATH` | `log.directory` | No file output |
| `--log-level FILTER` | `log.level` | `info` |
| `--log-timezone local\|utc` | `log.timezone` | `local` |
| `--log-ansi true\|false` | `log.ansi` | `true` (terminal colors; fields stay plain) |

ANSI styling affects only the console; files always remain plain text.
In v0.1.3 rotation dates are UTC. Version 0.1.4 and later use the selected
log timezone for rotation and filenames as described below.

```toml
[log]
level = "info"
console = true
directory = "/var/log/rosbridge"
rotation = "daily"
max_files = 7
```

File output still uses the bounded `tracing-appender` background queue. Version 0.1.4 and later name the active file `YYYYMMDDHHmm.logging` using its creation
time in `log.timezone`. Rotation or graceful shutdown closes it as
`YYYYMMDDHHmm.log`. For example:

```text
202609060240.log
202609060300.logging
```

`daily`/`hourly` rotate on the first write after the corresponding calendar
boundary in the selected timezone; `never` keeps one file for that process run.
Each restart opens a new file. If a timestamp already exists, `-1`, `-2`, etc.
are appended rather than overwriting it. Abnormal termination may leave
`.logging` files; they are preserved, not reused or pruned.

`max_files` retains that many completed timestamped `.log` archives, in addition
to active/unfinished files. It is not a size limit. Use a dedicated log directory;
retention ignores unrelated filenames, including the old `rosbridge_server_rs.log.*`
format. The writer drains and finalizes on graceful shutdown. Its default lossy
queue can drop lines if disk writes cannot keep up. Console output goes to stderr.
Ensure the directory is writable; in Docker, use a bind mount to preserve logs.

Handshake logs include a numeric connection ID, socket peer, URL path, Origin,
User-Agent and `forwarded_for`. Forwarding headers are client-supplied metadata,
not verified identity; behind Caddy the socket peer is the proxy. Session-end logs
include duration and the received close code/reason when available. Use the
connection ID to correlate these with subscription and error logs. No query
strings, cookies or Authorization headers are recorded.

Version 0.1.3 logs service calls and responses at INFO in both
forwarding directions, including connection, service, request ID and response
elapsed time. Timeouts are WARN; rejected or failed operations are ERROR with the
request ID and elapsed time. A received response does not imply application-level
success. Request and response payloads are not included in these INFO lifecycle logs.
In v0.1.4 and later, normal service calls and responses use DEBUG
instead, under `rosbridge_server_rs::service_calls`. INFO no longer includes periodic
rosapi call/response lifecycle lines. Timeouts remain WARN and errors remain ERROR.
Enable lifecycle details without payloads using
`--log-level 'info,rosbridge_server_rs::service_calls=debug'`.

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
| `incoming_queue_size` | Per-connection pending command capacity, default 256; connections are served round-robin. |
| `write_queue_size` | Supported as per-client queued topic batches, default 64; a batch may contain multiple frames. Topic congestion drops whole batches and preserves the connection. |
| `default_call_service_timeout` | Supported via `service_timeout`; retains Rust default 30 seconds. Positive values only. |
| `fragment_timeout` | Supported; retains Rust default 30 seconds. Expired assemblies are removed on the next received frame. |
| `topics_glob`, `topics_pub_glob`, `topics_sub_glob` | Forwarding allowlists for advertisements, publications and subscriptions, also used by native rosapi discovery. |
| `services_glob` | Allowlist for WebSocket service calls and advertisements, also used by native rosapi discovery. |
| `params_glob`, `params_timeout` | Parameter-name allowlist for native rosapi and WebSocket parameter calls; timeout defaults to 5 seconds. |
| `ssl`, `certfile`, `keyfile` | Not accepted; terminate TLS in a reverse proxy. |
| `use_compression` | Not accepted; WebSocket permessage-deflate is not implemented. Protocol CBOR/PNG remains supported. |
| `websocket_ping_interval`, `websocket_ping_timeout` | Supported in seconds; both default to 30. Set interval to 0 to disable active probes. |
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

### Service payload debugging

Since v0.1.3, enable service request/response previews with:

```bash
rosbridge_server_rs --log-level 'info,rosbridge_server_rs::service_payload=debug'
```

A global `--log-level debug` also enables them. Previews include connection,
service, request ID, direction and request/response kind. Each compact JSON preview
is limited to 4096 UTF-8 bytes and marked `truncated=true` when shortened; a
truncated preview is not necessarily valid JSON. Formatting stops at the limit
and is skipped entirely when this DEBUG target is disabled. Newlines in strings
are JSON-escaped. The message sent over ROS/WebSocket is unchanged.

These previews are not redacted and may contain credentials or private settings.
Startup logs warn when enabled. They use the same console/file destinations and
retention as other logs. INFO remains payload-free; restore it after debugging.
Only accepted, forwarded service requests and responses are previewed, after
parameter-name filtering on responses. Rejected requests do not emit previews.

## Outbound congestion

`write_queue_size` bounds queued topic batches per connection. Without a positive
subscription `queue_length`, a full topic queue drops the incoming batch. With
`queue_length > 0`, it also bounds queued batches for that topic and evicts that
topic's oldest batches to make room for newer ones, including when `throttle_rate`
is zero. Other topics are not evicted. If space still cannot be made, the incoming
batch is dropped. This transport-level use of `queue_length` is a Rust extension;
the existing subscription throttle queue remains separate. In-flight batches cannot
be replaced. All rosbridge fragments of a message are queued or dropped together.

Service requests/responses, action messages and status messages use a separate
16-batch reserve, drained before the next topic batch. Exhausting this reserve
closes the connection with WebSocket code 1013 and reason `control send queue full`,
rather than silently losing a response while leaving the client waiting.

`write_queue_bytes` (CLI `--write-queue-bytes`, default 67108864) limits estimated
retained input bytes in **each** queue: up to 64 MiB of topic input plus 64 MiB of
control input per client by default. Already encoded batches are charged by payload
length. The encoded result of each job is separately checked against the same
limit. A batch over either limit is dropped for topics or closes the connection
for control messages. The input estimate includes JSON/CBOR values, containers
and strings; it is not a total process-memory limit. Allocator overhead, codec
temporaries, subscription buffers and in-flight batches are additional.

Topic drops are counted per connection and logged at most once per second, with
topic, operation, batch capacity and queued bytes. Closed receivers, control queue
overflow, socket errors and write timeouts are reported separately. Reading and
writing run concurrently, so a blocked data write does not block ordinary incoming
commands. Each outgoing batch has a 10-second write deadline. Server-initiated
closes attempt a Close frame and wait up to one second for the peer; peer Close
frames are acknowledged. An already blocked or broken network can still prevent
delivery of the Close frame.

## Incoming fairness and heartbeats

`incoming_queue_size` now bounds pending protocol commands **per connection**,
not across the server. Each ready connection gets one command per turn, preserving
FIFO order within that connection. The worker handles at most 64 commands before
polling ROS events. A full client queue closes only that client (code 1013,
`client incoming queue full`); commands are not silently dropped. Disconnect and
shutdown notifications do not consume data capacity, and disconnect discards that
client's backlog. This provides command-count fairness, not preemption of a slow
individual operation. Total incoming capacity grows with the number of clients.

Active heartbeats default to a 30-second interval and 30-second pong timeout.
Configure `websocket_ping_interval` / `websocket_ping_timeout` in TOML, or
`--websocket-ping-interval` / `--websocket-ping-timeout` on the command line.
Interval 0 disables probes; the timeout must remain positive. The first ping is
sent after the interval, and the next interval starts after a matching pong.
Only a pong echoing the outstanding ping payload satisfies a probe; ordinary
messages and unrelated pongs do not postpone the deadline. The pong deadline
starts after the ping has been flushed, with a separate bounded ping-write wait.
On timeout the server logs the reason, releases the connection's output receiver,
and attempts a Close frame (1001, `WebSocket pong timed out`). Client-initiated
pings are still answered when active heartbeats are disabled.

## Outbound encoding workers

JSON/CBOR serialization, PNG compression and rosbridge fragmentation run through
Tokio blocking tasks with a shared concurrency limit. Set `encoding_workers`
(default 2, accepted range 1–256), or CLI `--encoding-workers`. A task is spawned
only after acquiring a permit. Connections waiting for capacity leave their
requests in their existing bounded queues, so topic drops and oldest-first
replacement happen before encoding. Each connection encodes at most one batch
at a time and retains FIFO order within each lane; control traffic takes priority
at batch boundaries. An already executing topic batch is not preempted.

Disconnect discards pending requests and wakes pool waiters. A synchronous codec
already running is allowed to finish, its result is discarded, and its permit is
held until CPU work ends even if the socket task is canceled. Encoding failures
are logged; control-message encoding failure closes with code 1011. An encoded
control batch exceeding the byte limit closes with 1013. Topic failures are counted
and logged at most once per second.

Enable `rosbridge_server_rs::outgoing=debug` to observe `queue_wait_us`, `encode_us`,
`input_bytes` and `output_bytes`. Native ROS message conversion (`msg.values()`),
protocol dispatch, input-size estimation and fan-out remain on the ROS worker.
This change does not introduce cross-client encoding caches. Actual latency and
CPU improvements require profiling with the deployed message sizes and codecs.
The [plan and review](outbound-encoding-plan.md) document the scope and tradeoffs.
