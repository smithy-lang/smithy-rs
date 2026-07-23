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
- Under Retry Behavior 2.1, `adaptive` retry mode now acquires a client-side rate-limiter send token before every send, including retries. Previously a retry computed a rate-limiter delay but sent without acquiring a token, so the adaptive rate limiter under-throttled retried requests. Pre-2.1 adaptive behavior — still the default today, until 2.1 becomes the default (see [aws-sdk-rust#1431](https://github.com/awslabs/aws-sdk-rust/discussions/1431)) — is unchanged.
- Honor the server-directed `x-amz-retry-after` header on responses that are retryable purely by HTTP status (e.g. a bare 500). This is enabled by a new `ClassifyRetry::classify_retry_v2`, which additionally receives the `RetryAction` accumulated by earlier-running classifiers and can refine it. It has a default implementation that delegates to `classify_retry`, so existing classifiers are unaffected.
