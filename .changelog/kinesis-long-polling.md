---
applies_to: ["aws-sdk-rust"]
authors: ["ysaito1001"]
references: [smithy-rs#5055]
breaking: false
new_feature: false
bug_fix: false
---
Kinesis `ReceiveQueueRecords` and `ReceiveDeadLetterQueueRecords` are now long-polling operations. When an error is retryable, they back off before the next attempt, even if the retry quota is exhausted.
