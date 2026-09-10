# rosbridge_server_rs 0.1.6

- Default file log rotation to twenty-minute boundaries (minute 00, 20 and 40),
  triggered by the first write after each boundary; configure with `rotation = "20min"`.
- Handle SIGTERM as well as SIGINT through graceful server shutdown so the log
  queue drains and active `.logging` files are finalized as `.log` archives.
- Test twenty-minute boundaries, normal log destruction and real process signal shutdown.

Existing explicit configuration files retain their rotation setting. Set
`rotation = "20min"` to enable the new interval. Managed default configuration is
refreshed on the version change. SIGKILL and power loss cannot finalize logs;
previous unfinished `.logging` files remain preserved.
