# aws-smithy-lang fuzz targets

Requires nightly Rust and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

- `idl_document`: arbitrary bytes as one IDL file.
- `idl_node`: arbitrary text as a node value, in metadata and as trait values. This focuses on
  numbers, quoted strings, text blocks, and nesting.
- `idl_mixed`: two IDL files (split at the first zero byte) assembled with a JSON AST document,
  exercising cross-input resolution.

Each input is assembled with default limits and again with low depth, shape, diagnostic, and
byte limits. Accepted models must round-trip through the JSON AST; errors must render.

Seed the corpus with the specification examples and differential fixtures:

```bash
cd rust-runtime/aws-smithy-lang/fuzz
mkdir -p corpus/idl_document
find ../tests/fixtures -name '*.smithy' -exec cp {} corpus/idl_document/ \;
cargo +nightly fuzz run idl_document -- -max_total_time=600
```
