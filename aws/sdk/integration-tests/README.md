# Handwritten Integration Test Root

This folder contains hand-written integration tests that are specific to individual services. In order for your test to be merged into the final artifact:

- The crate name must match the generated crate name, eg. `kms`, `dynamodb`
- Your test must be placed into the `tests` folder. **Everything else in your test crate is ignored.**

The contents of the `test` folder will be combined with codegenerated integration tests & inserted into the `tests` folder of the final generated service crate.
