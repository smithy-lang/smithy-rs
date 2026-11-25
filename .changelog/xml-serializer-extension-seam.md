---
applies_to:
- client
- server
authors:
- DarkIsDude
references:
- smithy-rs#4771
breaking: false
new_feature: false
bug_fix: false
---
`XmlBindingTraitSerializerGenerator` is now `open`, and the members it walks shapes with are `protected` instead of `private`, so a protocol defined outside `codegen-core` can subclass it rather than re-implement XML serialization. Visibility only: no behavior or generated-code change.
