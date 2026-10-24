---
applies_to: ["server"]
authors: ["xz0803"]
references: ["smithy-rs#4934"]
breaking: false
new_feature: false
bug_fix: true
---
Improve server routing performance for REST protocols. `RequestSpec` now shares its path regex across clones, so cloning the router per request (as `aws_smithy_http_server::serve` does) no longer throws away the regex match cache and rebuilds it on every request.
