---
applies_to:
- client
- aws-sdk-rust
authors:
- ysaito1001
references:
- smithy-rs#4638
breaking: false
new_feature: false
bug_fix: false
---
Implement retry behavior 2.1, gated behind `BehaviorVersion::v2026_05_15()`. Non-throttling errors now use 50ms base backoff (previously 1,000ms). Token bucket drain rate is 14 for non-throttling errors (previously 5) and 5 for throttling errors (previously 10 for timeouts). The `x-amz-retry-after` header is clamped between the computed exponential backoff and 5 seconds above it. Long-polling operations now backoff even when the token bucket is empty. HTTP 5xx responses no longer trigger connection poisoning. Timeouts are no longer treated differently from other transient errors.
