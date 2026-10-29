# Schema-serde development tools

## Generated-serde parity

`check-generated-serde-parity.py` checks that the schema-serde codegen checkout
preserves the existing generated-serde output. It generates matching HTTP 1.x
and HTTP 0.x server SDKs with schema serde disabled, then compares their Rust
sources and Cargo manifests. This is a manual development tool.

From the repository root, run:

```sh
python3 tools/schema-serde/check-generated-serde-parity.py
```

The script asks for the mainline repository, the schema-serde repository, and
the model. Both repository defaults point to this checkout; the default model
is `pokemon`, which selects both shared Pokémon model files.

The same inputs can be supplied as arguments:

```sh
python3 tools/schema-serde/check-generated-serde-parity.py \
  --mainline-repo /path/to/mainline \
  --schema-serde-repo /path/to/schema-serde \
  --model pokemon
```

When both repository paths identify the same checkout, the reference is generated
in a temporary detached worktree at the local `origin/main` reference. Refresh
that reference with `git fetch origin main` when needed, or select another local
revision with `--mainline-ref`. The temporary worktree is removed afterward.
Distinct repository paths use the code currently checked out in each repository.

For another model, pass an absolute path or a path relative to your current
working directory. A directory or comma-separated model files are also accepted.
Both generations read the same model inputs. If the model contains multiple
services, supply `--service namespace#ServiceName`.

The script prints the output directory containing generated SDKs and generation
logs. Exit code `0` means parity passed; `1` indicates differences or a generation
failure. Use `--help` for the options to compare existing generated SDKs.

## Manual server compatibility suites

The broad compatibility SDK matrix is excluded from default generation and CI.
Run all Kotlin, generated-protocol, and Rust integration comparisons with:

```sh
./gradlew serverCompatibilityTest
```

Use `:codegen-server:compatibilityTest` for only Kotlin comparisons,
`:codegen-server-test:compatibilityProtocolTest` for the standalone generated
protocol fixtures, or `:codegen-server-test:protocolCompatibilityTest` for the
Rust `integration-tests/protocol-compat` crate. Each Rust task enables its optional
SDK projections automatically. `generateServerCompatibilitySdk` generates the
optional server/client fixtures without running tests. Existing `-Pmodules=...`
selections continue to generate the requested compatibility SDKs.

The Rust compatibility crate remains in its existing directory with an independent
Cargo workspace and lockfile. The default integration workspace contains only the
event-stream and request-limit suites.
