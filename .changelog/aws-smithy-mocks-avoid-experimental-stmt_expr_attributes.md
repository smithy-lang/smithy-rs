---
applies_to:
- client
authors:
- eagletmt
references: []
breaking: false
new_feature: false
bug_fix: true
---

Currently, the following code does not compile because it requires an
experimental feature.
https://github.com/rust-lang/rust/issues/15701

```rust
let rule_builder = aws_smithy_mocks::mock!(aws_sdk_s3::Client::get_object);
```

Developers can workaround this issue by surrounding it with braces.

```rust
let rule_builder = { aws_smithy_mocks::mock!(aws_sdk_s3::Client::get_object) };
```

This PR adds the workaround to the aws-smithy-mocks crate.
