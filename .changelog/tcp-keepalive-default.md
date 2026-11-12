---
applies_to: ["client"]
authors: ["sfod"]
references: ["aws-sdk-rust#1296"]
breaking: false
new_feature: true
bug_fix: false
---
The connection pool now enables TCP keepalive by default with a 30-second idle interval. This keeps connections alive through NAT gateways and load balancers that silently drop idle TCP flows, reducing connection errors on reuse. Pass `None` to `tcp_keepalive` on the pool builder to opt out.
