$version: "2"

// Compilation probe for a query map with sensitive keys and values. Checking the generated
// Rust with warnings denied detects unused parser variables and verifies that redaction
// metadata can be emitted by both serde implementations.

namespace compat.compileprobe
use aws.protocols#restJson1
@restJson1
service Audit { version: "1", operations: [Probe] }
@http(method: "GET", uri: "/probe")
operation Probe { input := { @httpQueryParams values: Secrets } }
@sensitive string Secret
map Secrets { key: Secret, value: Secret }
// QuerySensitivity.closure uses a named parameter even when all keys and all
// values are sensitive; check this under the codegen test's deny-warnings build.
