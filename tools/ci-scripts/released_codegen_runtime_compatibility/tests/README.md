# Released codegen/runtime compatibility tests

These are lightweight unit tests for the released-codegen compatibility tooling. They use mocks and temporary files; they do not generate or compile client or server SDKs.

Run the suite from the repository root:

```bash
PYTHONPATH=tools/ci-scripts \
python3 -m unittest discover \
  -s tools/ci-scripts/released_codegen_runtime_compatibility/tests \
  -v
```

`PYTHONPATH=tools/ci-scripts` is required so Python can import the `released_codegen_runtime_compatibility` package.

To run the end-to-end check that generates protocol SDKs with published codegen and compiles them with current runtime crates, run:

```bash
./tools/ci-scripts/released-codegen-runtime-compatibility.py
```

Use `--codegen-version VERSION` to select an exact published codegen release instead of the latest release from Maven Central.
