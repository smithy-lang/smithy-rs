# Codegen Coverage

Measures JaCoCo **line coverage of the Kotlin codegen** (`codegen-core`, `codegen-server`,
`codegen-client`) achieved by running the `rust-server-codegen` and `rust-client-codegen`
Smithy build plugins over a corpus of Smithy models. The goal is that every reachable codegen
line is exercised by at least one model.

## Running

```bash
./gradlew :coverage:coverage
```

This:
1. Generates `build/smithy-build.json` from the models in `models/` (one projection per model file).
2. Runs `smithy build` in a JVM with the JaCoCo agent attached (`runCodegenCoverage`).
3. Produces the report at:
   - HTML: `build/reports/jacoco/html/index.html`
   - XML: `build/reports/jacoco/coverage.xml`

## Analyzing gaps

```bash
python3 report_gaps.py              # overall + per-package summary
python3 report_gaps.py --files 30   # worst files by missed-line count
python3 report_gaps.py --file RequestBindingGenerator.kt  # missed line numbers
python3 report_gaps.py --zero       # files with no coverage at all
```

## Adding models

Drop a `.smithy` file in `models/`. Directives in the leading comment lines control the
projection:

```
// coverage-service: com.example#MyService          (required: the service shape to generate)
// coverage-import: constraints.smithy               (optional, repeatable: extra model files,
//                                                    resolved against codegen-core/common-test-models
//                                                    then the repo root)
// coverage-codegen: "publicConstrainedTypes": false (optional: JSON spliced into "codegen" settings)
// coverage-plugins: server                          (optional: server, client, or both; default both)
```

The model file itself is always imported, so it can be a self-contained model or a
directive-only wrapper around imported/classpath models (e.g. the `aws.protocoltests.*`
suites discovered from the `smithy-aws-protocol-tests` jar).

To find which Kotlin code generates a piece of Rust, enable `// coverage-codegen: "debugMode": true`
and inspect the generated crates under `build/smithyprojections/coverage/<module>/`—each block
of generated Rust is annotated with the Kotlin file/line that produced it.

## Workflow for closing gaps

1. Run `./gradlew :coverage:coverage`.
2. `python3 report_gaps.py --files 30` to find the biggest gaps.
3. `python3 report_gaps.py --file <name>` + read the Kotlin source to learn which
   model feature (trait, protocol, codegen setting) triggers the missed lines.
4. Author a model in `models/` that uses that feature; re-run.
5. If the lines are unreachable from any model (dead flags, extension points, AWS-SDK-only
   decorators, defensive `throw` branches), record them in `UNREACHABLE.md` and — when the
   whole file is unreachable — add it to `coverageExclusions` in `build.gradle.kts`.

## Current status

Line coverage: **codegen-server 93.1%**, codegen-core 89.4%, codegen-client 85.3%.
See `UNREACHABLE.md` for code that no model can reach — for the server that accounts for
~280 of the 518 missed lines, so the practical ceiling is ~96–97% and ~97% of *coverable*
server lines are covered.
