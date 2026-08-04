# aws-smithy-xml fuzz harness

Schema-based XML codec fuzz coverage built on `cargo-fuzz` and `libfuzzer`.
Mirrors the layout of `aws-smithy-json/fuzz` and exercises the
[`aws_smithy_xml::codec`](../src/codec/) `XmlSerializer` / `XmlDeserializer`
pair against arbitrary inputs.

## Prerequisites

```bash
rustup install nightly
rustup component add llvm-tools --toolchain nightly
cargo install cargo-fuzz
```

## Targets

| Target | What it asserts |
|---|---|
| `schema_xml_deserialize` | Every `read_*` method must not panic on arbitrary bytes. Includes a stack-overflow guard that builds 256-deep nested XML and checks `read_struct` returns `Err` rather than crashing. Read consumers propagate errors to avoid spinning the deserializer on a stuck position. |
| `schema_xml_serialize` | The serializer's output is well-formed XML — valid UTF-8, tokenizes cleanly with `xmlparser`, and start/end tags balance. Catches escape bugs, missing attribute quoting, unescaped `&`/`<`/`>`/`"` in text or attribute content, and unescaped EOL chars per the [EOL-encoding SEP](../../../.kiro/xml-end-of-line-encoding.md). |
| `schema_xml_roundtrip` | A value serialized and then deserialized through the codec equals the original. Skips inputs containing XML 1.0 invalid characters (a representation gap, not a roundtrip bug), non-finite floats (Smithy XML's `NaN`/`Infinity` text form isn't accepted by scalar `read_*` — known asymmetry), and empty map keys. |
| `schema_xml_struct_reorder` | Permutes child-element order in the wire form before deserialization. The deserializer must dispatch members by element name, not position. |
| `schema_xml_attribute_mix` | A struct with two `@xmlAttribute` members and two body members round-trips correctly through the deferred-start-tag-flush state machine in the serializer. Asserts attributes appear only on the root element and never as child-element children. |

`read_document` is not fuzzed: REST XML doesn't support documents, and
`XmlDeserializer::read_document` returns `SerdeError` unconditionally per
the protocol spec.

## Running

```bash
# Run a single target until ctrl-c. Corpus and crash artifacts persist
# under ./corpus/<target>/ and ./artifacts/<target>/.
cargo +nightly fuzz run schema_xml_deserialize

# Time-bounded run (useful for CI smoke checks):
cargo +nightly fuzz run schema_xml_serialize -- -max_total_time=60

# Reproduce a saved crash:
cargo +nightly fuzz run schema_xml_roundtrip artifacts/schema_xml_roundtrip/crash-...
```

## Coverage

`show-corpus-coverage.sh` replays a target's corpus with coverage
instrumentation and dumps annotated source via `llvm-cov show`:

```bash
./show-corpus-coverage.sh                            # defaults to schema_xml_deserialize
./show-corpus-coverage.sh schema_xml_serialize
./show-corpus-coverage.sh schema_xml_roundtrip
./show-corpus-coverage.sh schema_xml_struct_reorder
./show-corpus-coverage.sh schema_xml_attribute_mix
```

## Layout

- `Cargo.toml` — the fuzz crate. Has its own `[workspace]` block so it
  doesn't collide with the parent runtime workspace.
- `fuzz_targets/schema_common.rs` — shared schemas, `FuzzValue` and
  the (de)serialization helpers used by all targets except
  `schema_xml_struct_reorder` and `schema_xml_attribute_mix`, which
  use hand-authored probe types.
- `fuzz_targets/schema_xml_*.rs` — one binary per target.
- `corpus/` (gitignored) — libfuzzer-managed input corpus.
- `artifacts/` (gitignored) — minimized crash reproducers.
- `coverage/` (gitignored) — instrumented binaries and `*.profdata`.
- `show-corpus-coverage.sh` — coverage-report helper.

## How the harness has been used

A `cargo +nightly fuzz run ... -- -max_total_time=60` smoke pass on
each of the five targets totals ~2M iterations and zero crashes
against the codec as currently checked in. One real bug was found in
the earliest run (a UTF-8 char-boundary panic in `find_element_slice`
on multi-byte content like Cyrillic letters); it's fixed in
[`src/codec/deserializer.rs`](../src/codec/deserializer.rs) with a
regression test (`find_element_slice_handles_multibyte_utf8`).

## Adding a new target

1. Create `fuzz_targets/<name>.rs` mirroring an existing target's
   shape (`#![no_main]` + `fuzz_target!(|...| {...})`).
2. Add a `[[bin]]` entry in `Cargo.toml` pointing to it.
3. Reuse `schema_common::*` where possible — the shared schemas and
   helpers are designed to be picked up across targets.
4. Run `cargo +nightly fuzz build <name>` to verify the harness
   compiles, then `cargo +nightly fuzz run <name> -- -max_total_time=60`
   for a smoke check before committing.
