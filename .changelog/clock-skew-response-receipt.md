---
applies_to: ["aws-sdk-rust"]
authors: ["vcjana"]
references: ["smithy-rs#4988"]
breaking: false
new_feature: false
bug_fix: true
---
Fix clock skew correction counting server processing time as clock skew. A long-running call, such
as a synchronous Lambda `Invoke` over 10 minutes, could make the next request on the same client
fail with `InvalidSignatureException`. The SDK now compares the response `Date` header to when the
response headers arrived instead of to the midpoint of the request.
