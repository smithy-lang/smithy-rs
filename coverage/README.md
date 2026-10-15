# Codegen Coverage

The corpus has two separate responsibilities: measure which Kotlin codegen lines execute,
and execute assertions against generated Rust. Line coverage does not establish protocol
compatibility. A line exercised under REST and another exercised under RPC do not prove their
combination works.

## Running

```bash
./gradlew :coverage:coverage
```

This records JaCoCo coverage, generates every server model with `schemaSerde=false` and `true`, retains client
projections, and runs `cargo test --quiet --lib --tests` on every generated crate. Generation
or compilation failures do not prevent later projections from running. The report and
per-stage logs are under `build/strict-tests/`; any failed, ignored, suppressed, missing, or
zero-assertion projection makes the command fail. There is no known-compilation-failure
allowance.

The strict run sets `SMITHY_RS_STRICT_PROTOCOL_TESTS=1` while generating tests. Original
protocol assertions execute without expected-panic annotations, disabled-test lists, or
hotfixed expectations. Models without protocol tests are explicitly **behavior unverified**.
Compiling their generated crates does not count as behavioral coverage.

To prepare and run separately, or use only cached Cargo dependencies:

```bash
./gradlew :coverage:prepareStrictCorpus
python3 coverage/run_tests.py --offline
```

For JaCoCo independently:

```bash
./gradlew :coverage:codegenCoverageReport
```

The JaCoCo report is at:
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

## Compatibility comparisons

The five `*StrictCompatibilityTest` classes in `codegen-server` build separate generated
servers and send the same requests through their actual HTTP service implementations.
Fixtures cover populated, empty, and absent structures/unions, ordinary document fields,
payloads, headers/status, and modeled errors. CBOR also exercises event-stream frames.
The `rpc_payload_*.smithy` models ensure the original RPC structured/union-payload
combination has native request and response assertions in the corpus.
The `rpc_response_bindings_*.smithy` models require RPC status 200 despite
`@http(code: 201)` and `@httpResponseCode` values, forbid leaked ETag headers,
and assert that HTTP-bound members remain in the JSON/CBOR document. These run
in both serde modes through the ordinary coverage command.

The five `multi_protocol_*.smithy` wrappers import the enriched common model and
explicit fixture services selecting its new operation families. They add ten
legacy/schema projections and 326 declared request/response assertions. Request
fixtures also seed the actual fuzz response scenarios. JSON-only profiles add
DocumentValues; unsupported families and original streaming operations outside
these fixture services are recorded in the selection manifest rather than counted
as covered. Native assertions retain explicit Document null and named-union XML
list failures.

```bash
./gradlew :codegen-server:test --tests '*StrictCompatibilityTest*'
```

Comparisons retain status, headers, body meaning, and handler inputs. Encoding normalization
must validate each document completely and validate each content length before comparing.
Differences remain failing tests; diagnostic classification does not waive them.

**`schemaSerde=false` in the working tree is not automatically a historical baseline.**
When shared codegen or runtime code has changed, both paths can share a regression or
behavior change. Preserve the same model/configuration and generate the historical server
from a clean revision. The clean CBOR baseline preparation and provenance are under
`validation/rpc-cbor-strict/`.

The old percentages described Kotlin generation in an older checkout with schema generator
classes excluded. They are not results for this branch. This corpus still needs fixtures
for every model without assertions and a broader interaction matrix. The strict runner
reports these gaps; it cannot infer full behavior from line coverage.
