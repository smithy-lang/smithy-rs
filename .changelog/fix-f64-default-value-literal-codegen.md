---
applies_to:
- client
- server
- aws-sdk-rust
authors:
- ysaito1001
references:
- smithy-rs#4729
breaking: false
new_feature: false
bug_fix: true
---

Fix codegen emitting integer literals for f64/f32 non-zero default value comparisons in serializers. When a Smithy model specifies a non-zero `@default` on a Double or Float member using a JSON integer (e.g., `1` instead of `1.0`), the generated "skip if default" check produced `if value != 1` which fails to compile in Rust. The fix appends `_f64`/`_f32` type suffixes to the rendered default value.
