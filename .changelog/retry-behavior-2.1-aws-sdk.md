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
Implement retry behavior 2.1, gated behind `BehaviorVersion::v2026_05_15()`. DynamoDB and DynamoDB Streams use a 25ms base backoff (instead of the general 50ms) and increase max attempts to 4 (from 3). Long-polling operations (SQS `ReceiveMessage`, SFN `GetActivityTask`, SWF `PollForActivityTask`/`PollForDecisionTask`) backoff even when the retry token bucket is empty.
