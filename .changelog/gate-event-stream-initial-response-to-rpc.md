---
applies_to:
- client
- server
references:
- smithy-rs#4435
authors:
- rcoh
breaking: false
new_feature: false
bug_fix: true
---

Gate event-stream `try_recv_initial` to RPC protocols (`awsJson`, `awsQuery`, `rpcv2Cbor`) on both the client fluent builder and the server protocol generator. Previously, all event-stream operations performed an unconditional `try_recv_initial`, which can hang indefinitely on REST-bound operations whose streams take a long time to produce their first event. This change is a stopgap; the planned permanent fix tracked in #4435 will instead surface initial messages only when explicitly requested.
