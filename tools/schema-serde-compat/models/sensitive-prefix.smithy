$version: "2"
namespace compat.compileprobe
use aws.protocols#restJson1
@restJson1
service Audit { version: "1", operations: [Probe] }
@http(method: "GET", uri: "/probe")
operation Probe { output := { @httpPrefixHeaders("x-meta-") values: Secrets } }
@sensitive string Secret
map Secrets { key: Secret, value: Secret }
// HttpBindingGenerator.renderPrefixHeader interpolates redactIfNecessary inside
// a Rust format string; a sensitive target produces nested unescaped quotes.
