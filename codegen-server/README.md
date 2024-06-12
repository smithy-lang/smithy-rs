# Smithy Rust Server Generator

Server-side Smithy code generator

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/server/overview.html)

## Project Layout

* `codegen-server`: Server-side code generation.
* `codegen-server-test`: Server-side Smithy test and validation generation.
  Common commands:
  * `./gradlew :codegen-server-test:test` Generate code and run tests against it
