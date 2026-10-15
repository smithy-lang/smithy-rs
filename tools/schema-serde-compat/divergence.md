# Accepted schemaSerde divergences

## C06: Duplicate JSON members followed by null

Status: accepted divergence. The user explicitly chose to retain the current schemaSerde behavior; no implementation fix is planned.

Applies to REST JSON, AWS JSON 1.0, and AWS JSON 1.1 request deserialization.

| Request body | Legacy generated server | schemaSerde |
| --- | --- | --- |
| `{"text":"a","text":null}` | Clears `text` | Retains `"a"` |
| `{"number":7,"number":null}` | Clears `number` | Retains `7` |
| `{"text":null,"text":"b"}` | Sets `text` to `"b"` | Sets `text` to `"b"` |

The schema JSON codec skips null-valued structure members before invoking the consumer. Its existing strictness and leniency settings do not change this behavior. The generated server deserializer also consumes optional-member nulls without clearing a previous assignment.

### Evidence

Fresh generation and execution on 2026-10-14 used the same model and inputs with `http-1x: true`, comparing `schemaSerde: false` (legacy) with `schemaSerde: true`.

- Model: `models/duplicate-json-null.smithy`.
- Runner: `python3 tools/schema-serde-compat/run_c06.py` from the repository root.
- Kotlin test: `codegen-server/src/test/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/DuplicateJsonNullCompatibilityTest.kt`.
- Observation directory: `.runs/C06-20261014T195643.384285Z/`.
- Result: 30 HTTP-response comparisons; six differences, consisting of the string/null and integer/null cases in each protocol. The other 24 comparisons matched, including null-before-value and invalid-type errors.

The reproduction currently asserts strict equality and therefore fails on these accepted differences. That failure records this divergence; it does not authorize changing the implementation. The runner freshly generates both server paths and does not depend on the recorded observation directory.

## C08: Overflowing JSON float and double values become infinity

Status: accepted divergence. On 2026-10-15, the user explicitly chose to retain schemaSerde's conversion of overflowing JSON float/double values to signed infinity; no implementation fix is planned. This decision concerns float/double overflow, not integer behavior.

Applies to REST JSON, AWS JSON 1.0, and AWS JSON 1.1 request deserialization.

| Request body | Legacy generated server | schemaSerde |
| --- | --- | --- |
| `{"float":1e400}` | HTTP 400; handler not called | HTTP 200; handler receives `Some(inf)` |
| `{"double":1e400}` | HTTP 400; handler not called | HTTP 200; handler receives `Some(inf)` |
| `{"double":-1e400}` | HTTP 400; handler not called | HTTP 200; handler receives `Some(-inf)` |

The reviewed [REST JSON specification](https://smithy.io/2.0/aws/protocols/aws-restjson1-protocol.html#non-numeric-float-and-double-serialization) and [AWS JSON specification](https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html#non-numeric-float-and-double-serialization) define explicit special-value strings (`"Infinity"`, `"-Infinity"`, and `"NaN"`), but do not explicitly prescribe handling of numeric overflow such as `1e400`. This is an accepted compatibility difference, not a claim that the specification mandates HTTP 400.

### Evidence

Fresh generation and execution on 2026-10-14 used identical models and inputs with `http-1x: true`, comparing `schemaSerde: false` (legacy) with `schemaSerde: true`.

- Runner: `python3 tools/schema-serde-compat/run.py --protocols rest-json aws-json-10 aws-json-11` from the repository root.
- Observation directory: `.runs/20261014T204419.010668Z/`; per-protocol `*-legacy.jsonl` and `*-schema.jsonl` contain status, headers, exact response body bytes, and handler observations.
- Cases: `body-float-1e400`, `body-double-1e400`, and `body-negative-overflow` in each protocol: nine comparisons confirmed the divergence.
- Legacy responds with `{}` for REST JSON and AWS JSON 1.0, and an empty body for AWS JSON 1.1. SchemaSerde responds with `{}` in all three protocols. Both paths use the protocol's JSON Content-Type; schemaSerde includes `Content-Length: 2`, while legacy REST JSON includes `X-Amzn-Errortype: SerializationException`.

The runner's strict comparison continues to report these accepted differences. That failure records the divergence; it does not authorize changing the implementation.

## C09: Exact integer precision for decimal JSON numbers

Status: accepted divergence. On 2026-10-15, the user explicitly chose to retain schemaSerde's exact integer preservation instead of reproducing legacy precision loss; no implementation fix is planned. This decision concerns integer precision when reading decimal JSON numbers, separately from C08's float/double overflow.

Applies to REST JSON, AWS JSON 1.0, and AWS JSON 1.1 request deserialization.

| Request body | Legacy handler receives | schemaSerde handler receives |
| --- | --- | --- |
| `{"long":9007199254740993.0}` | `9007199254740992` | `9007199254740993` |

### Evidence

The same fresh 2026-10-14 generation described under C08 tested `body-large-long` in all three protocols, using identical models and inputs with `http-1x: true` and only `schemaSerde` toggled. All three comparisons confirmed the values above in `.runs/20261014T204419.010668Z/*-legacy.jsonl` and `*-schema.jsonl`.

Both paths return HTTP 200 with identical response headers and exact body bytes (`{}`) within each protocol. The handler-observed `long` differs; raw diagnostic logs also differ. The frozen request corpora are in that run's `cases/` directory, and `differences.json` records the comparisons. Reproduce using the runner command listed under C08.

The runner's strict comparison continues to report this accepted precision difference. That failure records the divergence; it does not authorize changing the implementation.

## C10: Document nesting limits differ

Status: known divergence, deferred for later review. On 2026-10-15, the user chose to preserve current behavior and revisit this finding later. No implementation change is planned now; this is not a permanent resolution of the compatibility question.

Applies to Document fields in REST JSON, AWS JSON 1.0, and AWS JSON 1.1, and to REST JSON `@httpPayload` Documents. Some deeply nested values reach the legacy handler with HTTP 200 but receive HTTP 400 without invoking the schemaSerde handler.

Depth below counts Document array/object containers, excluding the enclosing operation input structure. A numeric leaf is `0`; an empty-container leaf counts as a container. These are the deepest accepted values at the tested boundaries:

| Document placement and leaf | Legacy | schemaSerde |
| --- | --- | --- |
| Field, numeric leaf | 255 | 126 |
| Field, empty array/object leaf | 256 | 127 |
| REST payload, numeric leaf | 255 | 127 |
| REST payload, empty array/object leaf | 256 | 128 |

Legacy `expect_document_inner` starts a separate counter at the Document root and rejects recursion depth 256. SchemaSerde's `JsonDeserializer::read_document` uses the codec's shared maximum depth of 128, counting an enclosing structure and each Document value, including scalar leaves. The distinction explains why the observed container boundaries differ from the constants.

The reviewed [Smithy Document definition](https://smithy.io/2.0/spec/simple-types.html#document) and JSON protocol specifications ([REST JSON](https://smithy.io/2.0/aws/protocols/aws-restjson1-protocol.html#json-shape-serialization), [AWS JSON 1.0](https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html#shape-serialization), [AWS JSON 1.1](https://smithy.io/2.0/aws/protocols/aws-json-1_1-protocol.html#shape-serialization)) describe the supported values but do not prescribe a numeric nesting limit. [RFC 8259 section 9](https://www.rfc-editor.org/rfc/rfc8259.html#section-9) explicitly permits nesting limits. This is a compatibility difference, not an established specification violation. Legacy's guard was introduced to prevent stack overflow in [PR #4685](https://github.com/smithy-lang/smithy-rs/pull/4685); any future change should retain bounded parsing.

### Evidence

Fresh generation and execution on 2026-10-15 used identical models and requests with `http-1x: true`, comparing `schemaSerde: false` with `schemaSerde: true`.

- Runner: `python3 tools/schema-serde-compat/run_c10.py`.
- Model: `models/document-depth.smithy`.
- Kotlin test: `codegen-server/src/test/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/DocumentDepthCompatibilityTest.kt`.
- Completed observation directory: `.runs/C10-20261015T095942.857412Z/`.
- Coverage: arrays, objects, mixed nesting, and empty-container leaves at depths 0/1 where applicable, 124–129, and 253–257. All six generated-server builds/tests succeeded. There were 252 paired comparisons and 115 differences; no recorded panics. Source fingerprints remained unchanged during execution.
- The run retains frozen inputs, generated servers, actual settings, runtime sources, Cargo lockfiles, build logs, per-protocol `*-legacy.jsonl` / `*-schema.jsonl`, `boundary-summary.json`, and `differences.json`. Observations include handler values, container paths/depths, status, all response header values, and exact response body bytes as `wire_hex`.

Where both accept, status, headers, response bytes, and handler values match. Success responses are HTTP 200 with `{}`, the protocol's Content-Type, and Content-Length 2. Depth rejection responses are HTTP 400 with the protocol's Content-Type and no Content-Length; REST JSON also includes `x-amzn-errortype: SerializationException`. The exact rejection body is `{}` for REST JSON and AWS JSON 1.0, and empty for AWS JSON 1.1. Where both reject, the compared responses match and neither handler runs.

The runner exits 1 to report the known differences; this does not indicate a generation/test failure or authorize an implementation change. Earlier incomplete attempts are retained separately; the completed directory above is the evidence for these boundaries. The Document-field null discrepancy under C07 remains separate and unresolved.
