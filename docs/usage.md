# Configuration and protocol notes

## Command-line options

| Option | Default | Purpose |
| --- | --- | --- |
| `--bind` | `0.0.0.0:9090` | WebSocket listen address |
| `--node-name` | `rosbridge_websocket` | ROS node name |
| `--namespace` | `/` | ROS node namespace |
| `--use-sim-time` | Disabled | Read time from `/clock` |
| `--no-rosapi` | Disabled | Use an independently managed rosapi node |
| `--service-timeout` | `30` | Service response and Action acceptance timeout, in seconds |
| `--max-message-size` | `16777216` | Maximum incoming message size, in bytes |

Arguments after `--` are forwarded to RCL:

```bash
./target/release/rosbridge_server_rs -- --ros-args -r __node:=bridge
```

ROS uses the configured `ROS_DOMAIN_ID` and `RMW_IMPLEMENTATION`. The tested environment is Linux ARM64 with the default RMW in the ROS 2 Jazzy container; other RMW implementations have not been verified.

## WebSocket example

```javascript
const ws = new WebSocket('ws://localhost:9090');
ws.onmessage = event => console.log(JSON.parse(event.data));
ws.onopen = () => {
  ws.send(JSON.stringify({
    op: 'subscribe', topic: '/chatter', type: 'std_msgs/msg/String'
  }));
  ws.send(JSON.stringify({
    op: 'advertise', topic: '/from_browser', type: 'std_msgs/msg/String'
  }));
  ws.send(JSON.stringify({
    op: 'publish', topic: '/from_browser', msg: {data: 'hello'}
  }));
};
```

## Limitations

This is an early implementation, not a complete replacement for every component of rosbridge_suite.

- The server starts the installed Python `rosapi_node` as a child, inheriting the
  sourced ROS environment. It provides `/rosapi/*` services and is stopped when
  the server exits. Install `ros-jazzy-rosapi`, or use `--no-rosapi` with an existing
  node. The server exits if its rosapi child terminates unexpectedly.
- Use absolute topic, service, and Action names. Relative names resolve from `/`; private names and full namespace semantics are not supported.
- TLS and authentication are not built in. Use a reverse proxy for WSS.
- Shared subscriptions use Python’s encoding precedence: CBOR-RAW, CBOR, PNG, then JSON. Like Python rosbridge, binary CBOR output ignores `fragment_size`.
- CBOR-RAW reserializes received messages to CDR; it does not preserve original network packets or provide zero-copy transport.
- `long double` and non-finite floating-point JSON inputs are not supported.
- Native ROS services cannot carry a browser-side failure as an exception response. Native callers should set a timeout.
- Full roslibjs compatibility, other ROS distributions, macOS native builds, production load, and long-running stability have not been verified.
