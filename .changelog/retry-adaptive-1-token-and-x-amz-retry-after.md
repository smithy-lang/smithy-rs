---
applies_to:
- client
authors:
- ysaito1001
references:
- smithy-rs#4749
breaking: false
new_feature: false
bug_fix: true
---
Fix two retry-behavior bugs:
- Under Retry Behavior 2.1, the adaptive client-side rate limiter now charges a uniform 1 token per send attempt (initial or retry). Pre-2.1 adaptive behavior is unchanged.
- Honor the server-directed `x-amz-retry-after` header on responses that are retryable purely by HTTP status (e.g. a bare 500). This is enabled by a new `ClassifyRetry::classify_retry_v2`, which additionally receives the `RetryAction` accumulated by earlier-running classifiers and can refine it. It has a default implementation that delegates to `classify_retry`, so existing classifiers are unaffected.
