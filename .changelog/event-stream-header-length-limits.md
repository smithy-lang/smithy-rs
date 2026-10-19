---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["vcjana"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Enforce the event stream protocol's header value length limit when encoding messages. String and byte array header values longer than 32,767 bytes are now rejected instead of encoded. `aws_sigv4::event_stream::sign_message` now returns an error instead of panicking when a message cannot be encoded.
