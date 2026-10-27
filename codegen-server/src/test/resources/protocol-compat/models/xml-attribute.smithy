$version: "2"

// Compilation probe for an XML attribute input. The source-driven suite checks generated
// Rust with warnings denied, pins the legacy unused-setter diagnostic, and requires schema
// generation to compile without that diagnostic.

namespace compat.compileprobe
use aws.protocols#restXml
@restXml
service Audit { version: "1", operations: [Probe] }
@http(method: "POST", uri: "/probe")
operation Probe { input := { @xmlAttribute token: String } }
// The legacy attribute parser leaves its generated set_token method unused.
