---
applies_to:
- aws-sdk-rust
authors:
- ysaito1001
references:
- smithy-rs#4638
breaking: false
new_feature: true
bug_fix: false
---
Implement retry behavior 2.1, opt-in via `AWS_NEW_RETRIES_2026=true` environment variable. Non-throttling errors now use 50ms base backoff (previously 1,000ms). Transient retry quota cost is 14 tokens (previously 5). DynamoDB and DynamoDB Streams use a 25ms base backoff and increase default max attempts to 4 (from 3). Long-polling operations (SQS `ReceiveMessage`, SFN `GetActivityTask`, SWF `PollForActivityTask`/`PollForDecisionTask`) backoff even when the retry token bucket is empty.
