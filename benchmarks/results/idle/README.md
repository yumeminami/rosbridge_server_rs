# Idle CPU comparison

Linux x86_64, ROS 2 Jazzy, `rmw_cyclonedds_cpp`, release build. Python includes
both rosbridge_websocket and rosapi_node; Rust includes its native rosapi services.
The ROS CLI daemon and other container processes are excluded from both totals.
CPU is expressed as a percentage of one logical core; memory is summed RSS.

![All CPU and memory trials](comparison-light.svg)

| State | Rust CPU median | Python CPU median |
| --- | ---: | ---: |
| Disconnected | 0.40% | 0.70% |
| Connected, no subscriptions | 0.50% | 0.50% |
| After unsubscribe | 0.40% | 0.80% |

Each state has three 10-second measurements. The server order alternates between
trials. Before measuring unsubscribe, the client subscribes to
`/rosbridge_idle_probe` (`std_msgs/msg/String`) and then cancels the subscription.
That topic has no publisher; these results do not measure active-stream CPU or
camera-preview performance. The connection remains open after unsubscribe.

The first trial had substantially higher CPU for both implementations. All samples
are retained in the plot and [cpu.json](cpu.json). The machine was running a live
robot ROS graph, so network/discovery activity was not held constant. The small
CPU differences should not be generalized into a broad performance claim.
Rust RSS was about 20–21 MiB; Python's combined RSS was about 145–153 MiB.

This is a separate x86_64 experiment from the historical ARM64 throughput benchmark.
Environment details are recorded in [metadata.json](metadata.json).

## Reproduce

Install the benchmark Python dependencies listed in the parent benchmark guide.
Source ROS and the robot's interface workspace. Run from the repository root on
the same machine, without another rosapi service provider:

```bash
cargo build --locked --release
python3 benchmarks/idle.py --seconds 10 --trials 3
python3 benchmarks/plot_idle.py
```

The script starts and stops its own servers, uses temporary WebSocket ports, and
keeps per-trial process logs. It does not stop other ROS nodes.
