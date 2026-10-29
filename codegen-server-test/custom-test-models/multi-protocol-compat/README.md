# AWS JSON response fixtures

This guide summarizes verification; it is not evidence that its expectations
are correct. Historical behavior is established by independently generated
mainline responses for the same actual values. Specification claims must be
checked against the published specification, with the Java implementation used
only for behavior it implements.

The operation shapes, request fixtures, and REST/CBOR response fixtures are shared
in `multi-protocol-common.smithy`. Import exactly one AWS JSON response file:

| Fixture file | `responseBindings` | Success | Modeled errors | Header-bound members |
| --- | --- | --- | --- | --- |
| `multi-protocol-awsjson-responses-historical.smithy` | `historical` (default) | `@http(code)` | `@httpError` | Headers and JSON body |
| `multi-protocol-awsjson-responses-specs.smithy` | `smithy-specs` | 200 | 400/500 by `@error` classification | JSON body only |

Both files contain the same 36 case IDs and `params`, covering AWS JSON 1.0 and
1.1. The historical expectations come from an independent mainline checkout's codegen and
runtime, with HTTP 1.x and no `schemaSerde` setting (mainline does not implement
it), using these exact operation shapes and values.
`awsjson-historical-observations.json` records the observed status, complete
headers, and body for each case. It also records the independently generated
`schemaSerde: false` responses from this checkout. All 36 match mainline in
status, complete headers (including content length), and JSON body content.
The snapshot contains the actual historical schema responses and specs-mode
responses as separate fields. The historical schema responses match both legacy
paths for all 36 cases.
JSON object member order is not significant.
An absent document (`None`) is omitted in both modes, producing `{}`. An explicit
null (`Some(Document::Null)`) produces `{"document":null}` in both modes.
Response-code members remain body data in both modes and do not override the
HTTP status. The snapshot records the mainline revision used for the reproduction.

The legacy observation services declared one AWS JSON protocol each. Unrelated
event-stream operations were omitted because legacy codegen rejects their
multi-member payloads. The generated observation tests constructed constrained
labels with `try_into()` and the empty modeled-default output with its generated
builder. For an omitted optional document, the observation test explicitly
constructed `None`: the legacy protocol-test instantiator otherwise constructs
`Some(Document::Null)`, which is a different value. These adaptations fixed
test-value construction without changing the
operation model, serializer, or expected responses. Observations were collected
before the original status assertions, which still failed against the specs
expectations.

`codegen-server-test/build.gradle.kts` defines two multi-protocol projections:

* `multi-protocol-server-sdk`: historical fixtures, 139 tests.
* `multi-protocol-server-sdk-specs`: specs fixtures, 139 tests.

`DocumentValues` is outside the all-five-protocol service because XML cannot
represent it. The `multi-protocol-document-server-sdk-{historical,specs}`
projections bind it to both AWS JSON protocols and REST JSON and run its twelve
response cases and twelve request cases in each mode. Together, the main and document projections
execute all 36 AWS JSON response cases in each mode.

The model guard counts only cases reachable from each server's bound operations
and checks that their protocols are declared by that server. Each main
multi-protocol projection executes 139 of them, including 28 AWS JSON response
cases. Each document projection executes another 24 tests, for 163 executable
cases across the two projections in each response mode.

An earlier reproduction incorrectly described an absent-document compatibility
gap because the legacy test instantiator supplied `Some(Document::Null)` while the
schema test supplied `None`. Mainline serialization of the same actual values
confirms compatibility; the serializers require no behavior change.

The document-payload review on
[PR 4894](https://github.com/smithy-lang/smithy-rs/pull/4894#discussion_r4255308407)
concerns request deserialization: a REST document payload whose body is JSON
`null` must produce `Some(Document::Null)`, rather than an unset member.
The author reported that fix in the same thread. The current generated
deserializer reads document values without the optional-member null check;
`null_struct_payload_is_rejected_like_legacy` checks that an empty document
payload stays unset and a JSON-null document payload stays present.
The response fixtures preserve this distinction: `None` is omitted and
`Some(Document::Null)` is serialized as JSON null. Correcting the earlier
test-value construction does not reverse the request-deserialization fix.

For these fixtures, the required historical comparison is:

| Behavior | Mainline legacy | Generated serde (`schemaSerde: false`) | Schema serde, historical | Schema serde, specs |
| --- | --- | --- | --- | --- |
| Modeled success status | 201 | 201 | 201 | 200 |
| Modeled client error with `@httpError(422)` | 422 | 422 | 422 | 400 |
| Header-bound members | Headers and body | Headers and body | Headers and body | Body only |
| Unset optional document | Omitted | Omitted | Omitted | Omitted |
| Explicit null document | JSON null | JSON null | JSON null | JSON null |

The mainline snapshot dictates the historical expectations. The other two
historical paths must match it for the same actual constructed values, not merely
the same Smithy `params` object.

The specification review uses the published
[AWS JSON 1.0](https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html#protocol-behaviors)
and [AWS JSON 1.1](https://smithy.io/2.0/aws/protocols/aws-json-1_1-protocol.html#protocol-behaviors)
rules: custom HTTP binding traits are ignored, and provided members go into the
JSON body. Error classification defaults are described in
[HTTP error status codes](https://smithy.io/2.0/spec/http-bindings.html#default-http-status-codes);
using 400/500 in specs mode combines these defaults with ignoring the custom
`@httpError` override. The AWS JSON protocol pages do not independently provide a
complete response-status table.

The Java reference revision is `9c5875c6195e6515538f6757b68e02295374c462`.
Its `AwsJsonProtocol` client expects 200 for successful responses, which
corroborates specs-mode success status. The Java JSON codec provides an additional
check of omitted members versus explicit document null. Its AWS JSON module is a
client implementation, so it is not used as a server oracle for the modeled error
status or response headers.

The `responseBindings` setting controls status and header bindings. Both modes
retain an empty JSON object for this modeled output when its document is unset.
The published AWS JSON empty-body section recommends an empty response body when
no output parameters are provided; this setting does not change that existing
empty-body policy.

Select the mode inside `plugins.rust-server-codegen` in `smithy-build.json`:

```json
{
  "codegen": { "http-1x": true, "schemaSerde": true },
  "customizationConfig": {
    "protocols": {
      "aws.protocols#awsJson1_0": { "responseBindings": "smithy-specs" },
      "aws.protocols#awsJson1_1": { "responseBindings": "smithy-specs" }
    }
  }
}
```

Use `historical` with the historical fixture file. Legacy generated serde always
uses historical behavior. Generated schema response tests pass the selected
protocol's settings to the registry, matching the generated service builder.
The fuzz harness imports historical response fixtures as seeds for legacy/schema
comparisons.

Run both modes:

```sh
./gradlew :codegen-server-test:assemble --quiet \
  -Pmodules=multi-protocol-server-sdk,multi-protocol-server-sdk-specs,multi-protocol-document-server-sdk-historical,multi-protocol-document-server-sdk-specs
cargo test --quiet \
  --manifest-path codegen-server-test/build/smithyprojections/codegen-server-test/Cargo.toml \
  -p multi-protocol-server-sdk -p multi-protocol-server-sdk-specs \
  -p multi-protocol-document-server-sdk-historical -p multi-protocol-document-server-sdk-specs --lib
```
