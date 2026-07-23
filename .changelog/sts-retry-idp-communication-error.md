---
applies_to:
- aws-sdk-rust
authors:
- ysaito1001
references:
- smithy-rs#4749
breaking: false
new_feature: false
bug_fix: true
---
STS clients now retry the `IDPCommunicationError` error code on all operations, not just those that model the `IDPCommunicationErrorException` shape.
