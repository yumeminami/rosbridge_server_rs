# Live browser workload

Measured on September 5, 2026, on the same Linux x86_64 host running ROS 2 Jazzy.
One browser WebSocket connection used Rust on port 8443; another used Python
rosbridge on port 9090. Both bridge nodes subscribed to
`/diagnostics` (`diagnostic_msgs/msg/DiagnosticArray`). Existing sessions and
servers were left running.

## Collection

[cpu.json](cpu.json) contains all 30 snapshots from the final observation, collected
with `docker stats --no-stream --format '{{json .}}'` for `rosbridge_server_rs`
and `rd001-rosbridge-1`. Each command sampled both containers and took approximately
two seconds. Timestamps mark the start of each command, not individual CPU counters.
Means and medians are calculated from the reported Docker CPU percentages; peaks
are the largest sampled values, not continuous maxima. Memory in the root README
is the final snapshot. The Python container includes rosbridge, rosapi and its ROS
launch process; the Rust container includes the native server and shell/diagnostic
processes. Container statistics include all processes, not only the servers.

## Interpretation

This observation found mean CPU of 3.637% for Rust and 19.8023% for Python plus
rosapi, an 81.6% reduction. One core at full utilization is 100%.
The graph preserves every sample, including the Python spikes.

An earlier sampling round overlapped ROS node inspection and contained diagnostic
CPU spikes. It was excluded from this container comparison. No ROS diagnostic
commands were run during the final round. The live ROS graph and browser request
schedules were not controlled; matching topic names does not establish identical
message delivery, QoS, throttling or rosapi request rates. No throughput, latency,
dropped-message or frame-loss measurements were collected.

Docker memory is not server RSS. A separate cgroup snapshot showed approximately
402 MiB of active file cache in the Rust container. In the earlier round, average
RSS was 29.5 MiB for the Rust process and 168.0 MiB summed across Python WebSocket
and rosapi processes, excluding the Python launch process. Those are different
measurements from the final Docker memory values and must not be compared directly;
summed RSS can also count shared pages more than once.

Regenerate the light and dark figures with `python3 benchmarks/plot_live.py`
(requires Matplotlib). To repeat the observation, keep one browser connected to
each server with the same topic selection and collect 30 successive Docker
snapshots. Record the client settings and surrounding ROS workload for each run.
