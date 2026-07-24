---
applies_to: ["client", "aws-sdk-rust"]
authors: ["jasgin"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Add support for configuring additional server names in TLS certificate verification.
When standard hostname verification fails, the client retries verification against
each configured additional server name. This is useful when a server presents a
certificate whose Subject Alternative Names (SANs) do not include the hostname used
to connect, but do include an alternative name the client has been configured to accept.
