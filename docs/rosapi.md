# Native rosapi

The server provides all 29 `rosapi_msgs` service interfaces inside its existing
Rust ROS node. No Python process or second ROS context is started. Services use
the `/rosapi` prefix; the owning node is the bridge node. Install
`ros-$ROS_DISTRO-rosapi-msgs` and source the ROS workspaces containing your interfaces.

| Area | Services |
| --- | --- |
| Topics | `topics`, `topics_for_type`, `topics_and_raw_types`, `topic_type`, `publishers`, `subscribers` |
| Services | `services`, `services_for_type`, `service_type`, `service_providers`, `service_node` |
| Nodes and interfaces | `nodes`, `node_details`, `interfaces` |
| Actions | `action_servers`, `action_type` |
| Type definitions | `message_details`, `service_request_details`, `service_response_details`, `action_goal_details`, `action_result_details`, `action_feedback_details` |
| Parameters | `get_param`, `set_param`, `has_param`, `delete_param`, `get_param_names` |
| Time and version | `get_time`, `get_ros_version` |

Both native ROS clients and WebSocket `call_service` clients can use these services.
Legacy `rosapi/Type` service names are accepted as `rosapi_msgs/srv/Type`.
Use `--no-rosapi` when another node already provides the services.

## Parameters and filtering

Parameter requests identify the destination as `/node_name:parameter_name`.
`set_param.value` and `get_param.default_value` contain JSON encoded as a string.
Parameter requests run asynchronously, so an unavailable node does not block topic
traffic or other clients. The default parameter timeout is five seconds.

ROS command-line parameter overrides configure the rosapi filters:

```bash
./target/release/rosbridge_server_rs -- --ros-args \
  -p 'topics_glob:=[/robot/*,/tf]' \
  -p 'services_glob:=[/robot/*]' \
  -p params_timeout:=5.0
```

Supported overrides are `topics_glob`, `topics_pub_glob`, `topics_sub_glob`,
`services_glob`, `params_glob`, and `params_timeout`. Empty strings allow everything;
`[]` allows nothing. Legacy topic globs are combined with the publisher and
subscriber globs. These control rosapi discovery results, not authorization of
WebSocket operations.

## Compatibility verification

`tests/test_rosapi.py` exercises every service against controlled ROS entities.
It compares results with the installed Python rosapi where that implementation
provides a usable reference, and checks parameter roundtrips, missing types,
constants and malformed requests separately.

Two Python rosapi 2.7.0 defects are deliberately not reproduced:

- `action_type` calls an unavailable rclpy method and crashes. The Rust test checks
  the known Action fixture's type directly.
- Type definitions include Python runtime properties and object addresses as
  constants. Rust returns the actual constants declared in the interface source.

Full message definition text uses the installed interface source and dependency
definitions. C introspection supplies field layout and defaults; definitions are
cached by type after the first request.
