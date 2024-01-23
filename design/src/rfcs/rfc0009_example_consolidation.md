RFC: Examples Consolidation
===========================

> Status: Implemented

Currently, the AWS Rust SDK's examples are duplicated across
[`awslabs/aws-sdk-rust`](https://github.com/awslabs/aws-sdk-rust),
[`smithy-lang/smithy-rs`](https://github.com/smithy-lang/smithy-rs),
and [`awsdocs/aws-doc-sdk-examples`](https://github.com/awsdocs/aws-doc-sdk-examples).
The `smithy-rs` repository was formerly the source of truth for examples,
with the examples being copied over to `aws-sdk-rust` as part of the release
process, and examples were manually copied over to `aws-doc-sdk-examples` so that
they could be included in the developer guide.

Now that the SDK is more stable with less frequent breaking changes,
the `aws-doc-sdk-examples` repository can become the source of truth
so long as the examples are tested against `smithy-rs` and continue to be
copied into `aws-sdk-rust`.

Requirements
------------

1. Examples are authored and maintained in `aws-doc-sdk-examples`
2. Examples are no longer present in `smithy-rs`
3. CI in `smithy-rs` checks out examples from `aws-doc-sdk-examples` and
   builds them against the generated SDK. Success for this CI job is optional for merging
   since there can be a time lag between identifying that examples are broken and fixing them.
4. Examples must be copied into `aws-sdk-rust` so that the examples for a specific
   version of the SDK can be easily referenced.
5. Examples must be verified in `aws-sdk-rust` prior to merging into the `main` branch.

Example CI in `smithy-rs`
------------------------

A CI job will be added to `smithy-rs` that:

1. Depends on the CI job that generates the full AWS SDK
2. Checks out the `aws-doc-sdk-examples` repository
3. Modifies example **Cargo.toml** files to point to the newly generated AWS SDK crates
4. Runs `cargo check` on each example

This job will not be required to pass for branch protection, but will
let us know that examples need to be updated before the next release.

Auto-sync to `aws-sdk-rust` from `smithy-rs` changes
--------------------------------------------------

The auto-sync job that copies generated code from `smithy-rs` into the
`aws-sdk-rust/next` branch will be updated to check out the `aws-doc-sdk-examples`
repository and copy the examples into `aws-sdk-rust`. The example **Cargo.toml** files
will also be updated to point to the local crate paths as part of this process.

The `aws-sdk-rust` CI already requires examples to compile, so merging `next` into `main`,
the step required to perform a release, will be blocked until the examples are fixed.

In the event the examples don't work on the `next` branch, developers and example writers
will need to be able to point the examples in `aws-doc-sdk-examples` to the generated
SDK in `next` so that they can verify their fixes. This can be done by hand, or a tool
can be written to automate it if a significant number of examples need to be fixed.

Process Risks
-------------

There are a couple of risks with this approach:

1. **Risk:** Examples are broken and an urgent fix needs to be released.

   **Possible mitigations:**

     1. Revert the change that broke the examples and then add the urgent fix
     2. Create a patch branch in `aws-sdk-rust`, apply the fix to that based off an older
        version of `smithy-rs` with the fix applied, and merge that into `main`.

2. **Risk:** A larger project requires changes to examples prior to GA, but multiple releases
   need to occur before the project completion.

   **Possible mitigations:**

     1. If the required changes compile against the older SDK, then just make the changes
        to the examples.
     2. Feature gate any incremental new functionality in `smithy-rs`, and work on example
        changes on a branch in `aws-doc-sdk-examples`. When wrapping up the project,
        remove the feature gating and merge the examples into the `main` branch.

Alternatives
------------

### `aws-sdk-rust` as the source of truth

Alternatively, the examples could reside in `aws-sdk-rust`, be referenced
from `smithy-rs` CI, and get copied into `aws-doc-sdk-examples` for inclusion
in the user guide.

**Pros:**
- Prior to GA, fixing examples after making breaking changes to the SDK would be easier.
  Otherwise, **Cargo.toml** files have to be temporarily modified to point to the
  `aws-sdk-rust/next` branch in order to make fixes.
- If a customer discovers examples via the `aws-sdk-rust` repository rather than via the
  SDK user guide, then it would be more obvious how to make changes to examples. At time
  of writing, the examples in the user guide link to the `aws-doc-sdk-examples` repository,
  so if the examples are discovered that way, then updating them should already be clear.

**Cons:**
- Tooling would need to be built to sync examples from `aws-sdk-rust` into
  `aws-doc-sdk-examples` so that they could be incorporated into the user guide.
- Creates a circular dependency between the `aws-sdk-rust` and `smithy-rs` repositories.
  CI in `smithy-rs` needs to exercise examples, which would be in `aws-sdk-rust`, and
  `aws-sdk-rust` has its code generated by `smithy-rs`. This is workable, but may lead
  to problems later on.

The tooling to auto-sync from `aws-sdk-rust` into `aws-doc-sdk-examples` will likely cost
more than tooling to temporarily update **Cargo.toml** files to make example fixes (if
that tooling is even necessary).

Changes Checklist
-----------------

- [x] Add example CI job to `smithy-rs`
- [x] Diff examples in `smithy-rs` and `aws-doc-sdk-examples` and move desired differences into `aws-doc-sdk-examples`
- [x] Apply example fix PRs from `aws-sdk-rust` into `aws-doc-sdk-examples`
- [x] Update `smithy-rs` CI to copy examples from `aws-doc-sdk-examples` rather than from smithy-rs
- [x] Delete examples from `smithy-rs`
