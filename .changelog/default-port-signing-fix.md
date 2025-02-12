---
applies_to: ["aws-sdk-rust"]
authors: ["landonxjames"]
references: ["aws-sdk-rust#1244"]
breaking: false
new_feature: false
bug_fix: true
---

Fix bug in Sigv4 signing that, when an endpoint contained a default port (80 for HTTP, 443 for HTTPS), would sign the request with that port in the `HOST` header even though Hyper excludes default ports from the `HOST` header.
