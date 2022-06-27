<!-- Do not manually edit this file. Use the `changelogger` tool. -->
v0.44.0 (June 23rd, 2022)
=========================
**Breaking Changes:**
- ⚠ ([smithy-rs#1484](https://github.com/awslabs/smithy-rs/issues/1484)) Server routing for URIs with prefixes has been fixed and now URIs are matched from the beginning
- ⚠ ([smithy-rs#1456](https://github.com/awslabs/smithy-rs/issues/1456)) Deprecated v1 arg format from sdk-versioner have been removed
- ⚠ ([smithy-rs#1424](https://github.com/awslabs/smithy-rs/issues/1424)) An error is raised if the http `Accept` header does not match the content type specified in smithy file

**New this release:**
- 🎉 ([smithy-rs#1454](https://github.com/awslabs/smithy-rs/issues/1454)) Python can be used for writing business logic, executing on top of smithy-rs Rust runtime

