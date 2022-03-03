cargo-api-linter
================

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

_Important:_ This tool requires a nightly build of Rust to be installed.

To install, run the following from this README path:

```bash
cargo install --path .
```

Then, in your library crate path, run:
```
cargo api-linter
```
