# Smithy Rust Server Generator

Server-side Smithy code generator

** This is a work in progress and generates serialization/de-serialization code that is probably unusable for the time being. **

[Design documentation (WIP)](https://smithy-lang.github.io/smithy-rs/)

## Project Layout

* `codegen-server`: Server-side code generation
* `codegen-server-test`: Server-side Smithy test and validation generation
  Common commands:
  * `./gradlew :codegen-server-test:test` Generate code and run tests against it
