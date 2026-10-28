---
applies_to: ["server"]
authors: ["xz0803"]
references: ["smithy-rs#4934"]
breaking: false
new_feature: false
bug_fix: false
---
Improve server routing performance for REST protocols. `RestRouter` now holds its `RequestSpec`s behind an `Arc`, so cloning the router shares each route's path regex and its match cache instead of rebuilding the cache: on every request for `aws_smithy_http_server::serve`, and on every connection for `aws-smithy-legacy-http-server`.
