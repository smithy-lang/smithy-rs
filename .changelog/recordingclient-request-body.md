---
applies_to:
- client
authors:
- signadou
references:
- smithy-rs#4495
breaking: false
new_feature: false
bug_fix: true
---
Fix `RecordingClient` request-body recording for non-streaming bodies by buffering request payloads in memory before recording, avoiding channel-based timing issues.
