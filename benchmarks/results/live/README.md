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
are the largest sampled values, not continuous maxima. The memory column in the root README uses process RSS from a separate round,
not Docker memory. The Python container includes rosbridge, rosapi and its ROS
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

## Process memory

[rss.json](rss.json) preserves all 30 process RSS samples from the earlier
61.74-second observation. RSS was read from `/proc/<pid>/stat` (resident pages
multiplied by the system page size). The README reports the arithmetic mean:

| Service processes | Mean RSS |
| --- | ---: |
| Rust server | 29.5 MiB |
| Python WebSocket + rosapi | 168.0 MiB |

The Python value sums the two service processes in each sample before averaging.
Shells, the Python launch process and ROS CLI diagnostic processes are excluded.
RSS counts resident process pages; summing processes can count shared pages more
than once, so this is not unique physical memory (PSS).

These samples came from the earlier round that overlapped ROS node inspection.
Its container CPU values were excluded, but the service RSS samples are retained
here with that context. RSS and the final CPU comparison were not collected in the
same interval. Docker memory values remain in the raw Docker snapshots for provenance
but are not presented as service memory usage.

Regenerate the light and dark figures with `python3 benchmarks/plot_live.py`
(requires Matplotlib). To repeat the observation, keep one browser connected to
each server with the same topic selection and collect 30 successive Docker
snapshots. Record the client settings and surrounding ROS workload for each run.
