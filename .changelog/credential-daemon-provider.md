---
applies_to: ["aws-sdk-rust"]
authors: ["osrs-agent"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Add a credential daemon credentials provider to `aws-config`. A profile in the shared config file that sets `daemon_credential = <name>` gets its credentials from a local credential daemon over a Unix domain socket. The provider is behind the new `credential-daemon` cargo feature, which is enabled by default. Credentials from the credential daemon are tracked with the `CREDENTIAL_DAEMON` feature ID.
