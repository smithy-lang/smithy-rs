---
applies_to:
- client
- aws-sdk-rust
authors:
- lauzadis
references: []
breaking: false
new_feature: false
bug_fix: true
---

Replace the `kotlin.streams.toList` extension on Java `Stream` with `java.util.stream.Collectors.toList()` in `OperationNormalizer` and `AwsPresigningDecorator`. The Kotlin extension collides with the JDK 16+ built-in `Stream.toList()` method, breaking compilation of the codegen plugins on builds that target newer JDKs. Behavior is unchanged on JDK 11.
