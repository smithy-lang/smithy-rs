---
applies_to:
- server
authors:
- lauzadis
references: []
breaking: false
new_feature: true
bug_fix: false
---

Support enum-typed members in server event stream unions. Previously, the server codegen rejected any model with an `enum`-trait shape reachable through an event stream, flagging it as an unsupported constraint. The `enum` trait is now excluded from that check, and the generated code is updated so it actually compiles and behaves correctly: event stream members skip the `MaybeConstrained`/builder unconstrained-type wrapping, and the generated unmarshaller calls `build()` on the parsed payload so a constraint violation surfaces as an unmarshalling error. Related: [smithy-lang/smithy#1388](https://github.com/smithy-lang/smithy/issues/1388).
