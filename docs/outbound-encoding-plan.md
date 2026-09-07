# Outbound encoding: implementation plan and review

## Objective

Move protocol serialization, compression and fragmentation out of the shared ROS
worker, and reject/replace congested topic batches before encoding them.

## Plan

1. Queue owned encoding requests alongside already encoded batches. Apply the
   existing topic and control capacity policies before encoding. Estimate owned
   input memory without serializing it and enforce the input byte budget.
2. Share an encoding pool across connections (default two concurrent jobs).
   Acquire a pool permit before removing a queued request and before spawning a
   blocking task. Each connection has at most one encoding request in flight.
3. Preserve FIFO order within the topic/control lanes and existing control
   priority between batches. Check encoded payload size before socket delivery.
4. Wake permit waiters on closure, discard queued requests, and discard results
   of an already running job after closure. Keep permits inside blocking jobs so
   cancellation cannot release capacity before CPU work finishes.
5. Add queue-wait and encoding-duration debug logs; verify deferred execution,
   saturation, ordering, shutdown/cancellation and JSON/CBOR/PNG fragmentation.

## Review decisions

- A task is not spawned per incoming ROS event. The existing per-connection queue
  is the only pending-job queue; semaphore waiters do not own removed payloads.
- Native ROS handles and `msg.values()` stay on the ROS worker. Jobs contain owned
  Rust values only. Moving raw ROS conversion requires a separate ownership design.
- Cross-client encoding caches are deferred: their memory accounting and fragment
  IDs need an independent design. This change reduces ROS-thread blocking and
  avoids encoding discarded batches, without claiming lower CPU for all traffic.
- The byte limit is applied to estimated retained input and separately to the
  encoded batch. It is not an RSS limit; codec temporaries and in-flight batches
  remain additional allocations. The configured worker count bounds concurrent
  codec work. Input accounting includes JSON/CBOR container and string storage.
- Synchronous `try_recv` remains available for protocol tests/non-async consumers;
  production sockets use asynchronous `recv` and the shared bounded pool.
- Running synchronous codecs cannot be preempted. On disconnect their eventual
  results are dropped, and their permits remain held until completion.

## Validation

Run all non-ROS tests and Clippy; check the ROS server wiring with Humble doc-only
bindings. A real ROS workload is still required to measure latency/CPU gains.

## Implementation review result

Implemented deferred encoding with permits acquired before dequeue, including
when a control message arrives while a topic waits. Cancellation tests confirm
that running jobs retain permits and queued jobs remain replaceable. All 61
non-ROS tests and Humble doc-only Clippy passed. Real ROS performance measurements
remain a deployment follow-up.
