cargo-check-external-types
==========================

Static analysis tool that detects external types used in a Rust library's public API.
Configuration can be provided to allow certain external types so that this tool can
be used in continuous integration so that types don't unintentionally make it into
the library's API. It can also output a Markdown table of the external types it found.

Example Output
--------------

The test suite has a Rust library that [relies on some external types](test-workspace/test-crate/src/lib.rs).
When the tool is run against this library without any configuration,
[it emits errors](tests/default-config-expected-output.txt)
for each occurrence of an external type in the public API.

When [a config file](tests/allow-some-types.toml) is provided,
the allowed external types [no longer show up in the output](tests/allow-some-types-expected-output.txt).

When the output format is set to `markdown-table`, then
a [table of external types](tests/output-format-markdown-table-expected-output.md) is output.

How to Use
----------

_Important:_ This tool requires a nightly build of Rust to be installed since it relies on rustdoc JSON output.
It was last tested against nightly-2022-07-25.

To install, run the following from this README path:

```bash
cargo install --locked cargo-check-external-types
```

Then, in your library crate path, run:
```bash
cargo +nightly check-external-types
```

This will produce errors if any external types are used in a public API at all. That's not terribly useful
on its own, so the tool can be given a config file to allow certain types. For example, we can allow
any type in `bytes` with:

```toml
allowed_external_types = [
    "bytes::*",
]
```

Save that file somewhere in your project (in this example, we choose the name `external-types.toml`), and then
run the command with:

```bash
cargo +nightly check-external-types --config external-types.toml
```

License
-------

This tool is distributed under the terms of Apache License Version 2.0.
See the [LICENSE](LICENSE) file for more information.
