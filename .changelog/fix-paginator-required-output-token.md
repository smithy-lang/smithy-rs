---
applies_to:
- client
- aws-sdk-rust
authors:
- vcjana
references: []
breaking: false
new_feature: false
bug_fix: true
---

Fix paginator codegen for operations whose `@paginated` `outputToken` targets a `@required` member. Previously, the generated `src/lens.rs` borrowing accessor emitted a direct field access (`input.field`) instead of a reference (`&input.field`) for required members, causing a type mismatch (`Option<&String>` vs `String`).
