$version: "2"
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
