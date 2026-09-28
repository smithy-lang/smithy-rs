Test base archive
=================

The `test_base.git.tar.gz` is an archived test git repository that looks like a
smithy-rs repo (with only the runtime crates), with some version numbers and
release tags.

It is a bare git repository (no working tree). To modify it, use the Makefile in
this directory to unpack it with `make unpack`. This will create a test_base directory
in test_data where you can make any changes you need to the repo. Then running
`make pack` will convert that modified repository back into the archive.

When making new test cases that need some extensive setup, it's best to create
a test-case specific branch in the test base.

For test cases that only need different version numbers, prefer rewriting them in
the cloned working tree with `TestBase::set_crate_version` instead of adding a
branch. The audit compares the working tree against a release tag, so uncommitted
changes are enough, and the `main` branch is byte-identical to the
`release-2023-10-02` tag.

Fake crates.io indexes
======================

The `*.toml` files in this directory are fake crates.io indexes, selected with
`--fake-crates-io-index`. The `[crates]` table lists published versions:

```toml
[crates]
aws-config = ["1.0.0"]
```

An optional `[crate_version_metadata]` table attaches the dependency requirements
published for a specific version, which is what the dependency-requirement audit
evaluates:

```toml
[crate_version_metadata."aws-config@1.0.0"]
yanked = false
dependencies = [
  { alias = "aws-smithy-json", requirement = "^0.60.0" },
]
```

`package`, `kind` (`normal`, `dev`, or `build`; defaults to `normal`), `target`,
and `optional` may also be set per dependency. A version listed in `[crates]`
without metadata has no dependencies and is not yanked.
