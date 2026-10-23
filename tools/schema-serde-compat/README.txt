Server schemaSerde compatibility audit
======================================

Run from the repository root:

    python3 tools/schema-serde-compat/run.py

Prerequisites: the repository's Java/Gradle and Rust toolchains, Python 3, and
access to the dependencies used by ordinary codegen integration tests. This
audit was executed with rustc 1.94.1 and Java 17. No extra Python packages are
needed. Paths are resolved from the script, so the checkout location can change.

The reference is CURRENT Kotlin code with schemaSerde=false and HTTP 1.x.
The candidate is the same source with schemaSerde=true and HTTP 1.x. Existing
changes to the legacy generator are therefore part of the reference. This is
not a historical release comparison. No deleted Markdown or previous audit
artifacts are used as evidence.

The runner generates both services from the same model, compiles and executes
the same requests through their real routers and handlers, and compares:

  * handler values, including unset versus explicit null;
  * status, all response header values, parsed JSON body and exact body bytes;
  * event receiver results and error details;
  * the streaming-blob input field's public type;
  * captured TRACE/DEBUG/ERROR logging with InstrumentPlugin installed;
  * forbidden synthetic secret markers in selected sensitive-model cases;
  * separate Rust compilation probes under -D warnings.

Exit status 0 means all recorded comparisons match, no recorded panic or
sensitive-marker failure occurred, and all compilation probes succeeded.
Exit status 1 means discrepancies or compilation-probe failures were found.
Exit status 2 means the audit was incomplete or its controls failed. Existing
differences are not silently blessed or hidden behind expected-failure tests.

Useful commands:

    python3 tools/schema-serde-compat/run.py --protocols rest-json rest-xml
    python3 tools/schema-serde-compat/run.py --compare /absolute/path/to/completed/run
    python3 -m unittest discover -s tools/schema-serde-compat -p test_runner.py

The fresh models are in models/. The request corpora are in cases/. Each case
has a stable ID and a Kotlin/runtime source rationale. bindings-and-json.smithy
contains shared REST/JSON shapes. cbor-shapes.smithy omits the Document shapes
rejected by rpcv2Cbor model validation. Each protocol has its own small service
model. The sensitive-prefix, sensitive-query-map and xml-attribute files are
minimal compilation probes.

SourceDrivenSchemaSerdeCompatibilityTest.kt is the only addition outside this
directory. It uses the existing serverIntegrationTest framework. Ordinary
Gradle test runs skip it unless invoked through the runner's environment.
The Rust harness templates are test code and do not alter generated production
behavior. The CBOR legacy streaming-blob observer uses Blob::as_ref, while the
schema observer collects ByteStream; both the bytes and this API difference
are explicitly recorded.

Every invocation gets a fresh .runs/<UTC timestamp>/ directory. It contains
the frozen corpus, generated Rust crates and Cargo.lock files, cargo/Gradle
logs, both JSONL observation files per protocol, differences.json, compiler
probe outputs, source fingerprints and toolchain/provenance metadata. The
shared Cargo target cache is .runs/target unless CARGO_TARGET_DIR is supplied.
No snapshot from an earlier invocation is used as the expected behavior.

Two explicit build accommodations are recorded:

  1. The repository's deleted Markdown files include compile-time Rust doc
     inputs. The runner copies current runtime sources into the run directory,
     excludes Markdown, and replaces #[doc = include_str!(...md)] attributes
     in that isolated copy with literal documentation. It never restores or
     reads the deleted files or changes the working runtime. The exact omitted
     attributes are listed in documentation-omissions.json.

  2. Legacy XML attribute parsing generates an unused setter. XML runtime
     observations use -D warnings -A dead_code so requests can execute. The
     independent xml-attribute probe still checks -D warnings and records the
     legacy failure. All other runtime observations also use -D warnings.

Results from the completed fresh audit
-------------------------------------

891 protocol/case comparisons were executed: REST JSON 283, AWS JSON 1.0 148,
AWS JSON 1.1 148, RPC CBOR 136, and REST XML 176. There were 79 case comparisons
with differences beyond log text. Six of those differ only in JSON response
member ordering after excluding logs. The missing legacy routing log causes
raw-log differences even for otherwise equivalent successes; all 891 raw
observations therefore differ. Do not interpret 891 as a bug count.

findings.json documents 17 behavior/public-API discrepancy groups, including
the supplied trailing-slash example (16 additional groups), three diagnostic
groups, a JSON byte-order group, and three compilation groups. evidence.json
contains actual representative old/new observations and compiler output.

The behavior/API groups are:

  C01  Trailing URI slash: routed requests rejected, or greedy value truncated.
  C02  Prefix-header string whitespace is preserved instead of trimmed.
  C03  Prefix-header media strings bypass base64 and UTF-8 validation.
  C04  Prefix-header media strings bypass base64 encoding on output.
  C05  Timestamp-list query format precedence differs.
  C06  Duplicate JSON member followed by null keeps the earlier value.
  C07  Document null becomes absent, including REST document payloads.
  C08  Overflowing JSON numbers are accepted as infinity.
  C09  Large integral decimal numbers retain precision lost by legacy.
  C10  Document nesting limits differ.
  C11  XML elements can set members modeled as attributes.
  C12  XML map entries missing a key/value are dropped instead of rejected.
  C13  Multiple scalar XML union variants are rejected instead of last-wins.
  C14  Modeled XML errors gain a Content-Type header for empty-output operations.
  C15  Event payload cannot overwrite an event-header member as legacy permits.
  C16  Corrupt initial CBOR event frame produces different HTTP error bytes.
  C17  RPC CBOR streaming blob input changes from Blob to ByteStream.

These are compatibility observations, not 17 claims of worse behavior.
Preserving precision, isolating event headers, and avoiding the legacy empty
media-header panic can be desirable differences. The sensitive prefix-header
output compilation failure is legacy-only; the all-sensitive query-map lint
failure is shared. No production fixes were applied.

Sensitive logging
-----------------

ServerOperationGenerator uses ServerHttpSensitivityGenerator on both paths.
InstrumentPlugin consumes that shared Sensitivity implementation: header
values/key suffixes, ordinary/greedy labels, query values/keys and response
formatting are generated from the model. InstrumentOperation logs the request
URI/headers and response status/headers, not the body. Generated structure
Debug redaction also comes from shared codegen.

Legacy request rejection additionally logs through the generated FromRequest,
request/mod.rs and operation/upgrade.rs. Schema DynUpgrade returns from the
corresponding rejection branches without those diagnostics. Response
serialization failures log on both paths, but legacy OperationBuildError and
schema SerdeError use different messages and redaction markers.

The tested sensitive request, response-header and modeled-error cases exposed
none of their forbidden synthetic markers in either path's captured logs.
This is bounded evidence, not a blanket guarantee for every sensitive model.
Unannotated values in observations are intentionally visible test data.

Extending the audit
-------------------

Start with a Kotlin branch and record its corresponding runtime branch in
coverage.csv/findings.json. Add the smallest model that selects it. Add cases
on both sides of the boundary, including a valid control whose expected
legacy status ensures that the request actually reaches the intended path.
If the new operation needs custom handler observations or outputs, extend
the new Kotlin test and the observer templates. Regenerate both implementations
and retain their direct observations. Keep a shared legacy defect distinct
from a regression introduced by schemaSerde.

coverage.csv explicitly lists tested, partial and untested areas. In
particular this suite does not certify multi-protocol arbitration, third-party
protocols, HTTP 0.x, output event streams, every customization option, live
backpressure/cancellation/trailers, or every model/trait combination. Finite
tests cannot establish 100% compatibility or count all remaining defects.

All additions for transfer are enumerated in ADDED_FILES.txt. Existing source
changes and Markdown deletions were preserved. Copy the listed files, or apply
the separately supplied additions-only patch, then rerun on the target machine.
