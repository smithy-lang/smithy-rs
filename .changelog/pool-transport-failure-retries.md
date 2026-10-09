---
applies_to: ["client", "aws-sdk-rust"]
authors: ["aajtodd"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
The opt-in connection pool now retries a connection that fails in the network before any request is sent: a refused or reset connect, a local socket error, a DNS failure, or the connection closing during the TLS handshake. These failures were not retried at all. A TLS rejection, a proxy's refusal, and a configuration error still fail at once.
