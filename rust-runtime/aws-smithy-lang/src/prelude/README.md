`prelude-1.74.1.json` is the `smithy.api` prelude serialized as Smithy 2.0 JSON AST by
`software.amazon.smithy:smithy-model:1.74.1` (`ModelSerializer` with `includePrelude(true)` and a
`smithy.api` namespace filter). It uses neither `apply` nor mixins.

`tests/fixtures/java/Regenerate.java` writes it, along with the Java compatibility fixtures; see
`tests/fixtures/java/README.md`. After moving to another Smithy version, delete the old file and
update `SOURCE_VERSION` and the `include_str!` path in `src/prelude.rs`.
