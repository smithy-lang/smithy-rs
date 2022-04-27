# Design FAQ

## What is Smithy?

Smithy is the interface design language used by AWS services. `smithy-rs` allows users to generate a Rust client for any
Smithy based service (pending protocol support), including those outside of AWS.

## Why is there one crate per service?

1. **Compilation time:** Although it's possible to use cargo features to conditionally compile individual services, we
   decided that this added significant complexity to the generated code. In Rust the "unit of compilation" is a Crate,
   so by using smaller crates we can get better compilation parallelism. Furthermore, ecosystem services like `docs.rs`
   have an upper limit on the maximum amount of time required to build an individual crate—if we packaged the entire SDK
   as a single crate, we would quickly exceed this limit.

2. **Versioning:** It is expected that over time we may major-version-bump individual services. New updates will be pushed
   for _some_ AWS service nearly every day. Maintaining separate crates allows us to only increment versions for the
   relevant pieces that change. See [Independent Crate Versioning](./rfcs/rfc0012_independent_crate_versioning.md) for
   more info.

## Why don't the SDK service crates implement `serde::Serialize` or `serde::Deserialize` for any types?

1. **Compilation time:** `serde` makes heavy use of [several crates](https://crates.io/crates/serde_derive/1.0.136/dependencies)
   *(`proc-macro2`, `quote`, and `syn`)* that are very expensive to compile. Several service crates are already quite large
   and adding a `serde` dependency would increase compile times beyond what we consider acceptable. When we last checked,
   adding `serde` derives made compilation 23% slower.

2. **Misleading results:** We can't use `serde` for serializing requests to AWS or deserializing responses from AWS because
   both sides of that process would require too much customization. Adding serialize/deserialize impls for operations has
   the potential to confuse users when they find it doesn't actually capture all the necessary information (like headers and
   trailers) sent in a request or received in a response.

In the future, we may add `serde` support behind a feature gate. However, we would only support this for operation `Input`
and `Output` structs with the aim of making SDK-related tests easier to set up and run.
