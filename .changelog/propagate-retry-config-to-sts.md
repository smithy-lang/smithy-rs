---
applies_to:
- aws-sdk-rust
authors:
- ysaito1001
references: []
breaking: false
new_feature: false
bug_fix: false
---
Propagate the customer-configured `RetryConfig` (e.g., `AWS_MAX_ATTEMPTS`) to the inner STS client used by credential providers in the default chain. Previously, the inner STS client always used the default retry configuration (3 attempts), ignoring the outer retry configuration. Now, if a customer sets `AWS_MAX_ATTEMPTS=5`, the inner STS client will retry up to 5 times.
