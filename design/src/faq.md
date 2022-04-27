# Design FAQ

### What is Smithy?

Smithy is the interface design language used by AWS services. `smithy-rs` allows users to generate a Rust client for any
Smithy based service (pending protocol support), including those outside of AWS.

### Why is there one crate per service?

1. Compilation time: Although it's possible to use cargo features to conditionally compile individual services, we
   decided that this added significant complexity to the generated code. In Rust the "unit of compilation" is a Crate,
   so by using smaller crates we can get better compilation parallelism. Furthermore, ecosystem services like `docs.rs`
   have an upper limit on the maximum amount of time required to build an individual crate—if we packaged the entire SDK
   as a single crate, we would quickly exceed this limit.

2. Versioning: It is expected that over time we may major-version-bump individual services. New updates will be pushed
   for _some_ AWS service nearly every day. Maintaining separate crates allows us to only increment versions for the
   relevant pieces that change. See [Independent Crate Versioning](./rfcs/rfc0012_independent_crate_versioning.md) for
   more info.
