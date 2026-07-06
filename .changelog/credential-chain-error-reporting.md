---
applies_to:
- aws-sdk-rust
authors:
- ysaito1001
references: []
breaking: false
new_feature: false
bug_fix: true
---

Improve default credential chain error reporting and align missing-profile behavior with other SDKs. When all providers in the chain are exhausted, the error now includes a per-provider summary showing why each was skipped. Additionally, providers whose source is not applicable no longer hard-fail the chain; they are skipped and the chain continues to the next provider. Here are the cases affected:

- `AWS_PROFILE` refers to a profile that does not exist in `~/.aws/config`
- The selected profile is configured for the token provider chain (has `sso_session` but no `sso_account_id`/`sso_role_name`)
- `AWS_WEB_IDENTITY_TOKEN_FILE` is set but `AWS_ROLE_ARN` is not
