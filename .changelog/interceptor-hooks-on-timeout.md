---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4318"]
breaking: false
new_feature: false
bug_fix: true
---
Interceptor finalizer hooks now run when an operation or attempt timeout elapses. `modify_before_attempt_completion`, `read_after_attempt`, `modify_before_completion`, and `read_after_execution` are documented to always run, but a timeout cancelled the future they lived inside, so they were skipped: an attempt timeout dropped the attempt-level pair, an operation timeout landing mid-attempt dropped all four, and one landing during retry backoff dropped the execution-level pair. Each hook now also observes the timeout rather than an empty context, so the two mutating finalizers can act on it. Hook invocation on a plain transport error is unchanged.

Consumers that compensated for the missing invocations may now see different counts, since the hooks fire where they previously did not. A finalizer that returns an error will also now surface that error to the caller on a timeout, in place of `SdkError::TimeoutError`.
