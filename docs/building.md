# Build from source

Requires Linux, ROS 2 Jazzy, and Rust 1.93 or later. On Ubuntu, install the
native build dependencies:

```bash
sudo apt update
sudo apt install build-essential clang libclang-dev pkg-config libssl-dev ros-jazzy-rosapi-msgs
```

Install the ROS interface packages used by your application, including their C
typesupport and introspection libraries.

```bash
source /opt/ros/jazzy/setup.bash
# Source your ROS workspace here if you use custom interfaces.
cargo build --locked --release
./target/release/rosbridge_server_rs --bind 127.0.0.1:9090
```


See [development checks](../README.md#development) for test commands.
