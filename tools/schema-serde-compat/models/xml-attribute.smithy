$version: "2"
namespace compat.compileprobe
use aws.protocols#restXml
@restXml
service Audit { version: "1", operations: [Probe] }
@http(method: "POST", uri: "/probe")
operation Probe { input := { @xmlAttribute token: String } }
// The legacy attribute parser leaves its generated set_token method unused.
