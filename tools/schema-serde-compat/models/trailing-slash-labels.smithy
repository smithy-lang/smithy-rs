$version: "2"
namespace compat.labels

use aws.protocols#restJson1
use aws.protocols#restXml

@restJson1
service JsonLabels {
    version: "1"
    operations: [Plain, Trailing, Greedy, GreedyTrailing, Suffix, Query]
}

@restXml
service XmlLabels {
    version: "1"
    operations: [Plain, Trailing, Greedy, GreedyTrailing, Suffix, Query]
}

// Kotlin's ServerHttpBoundProtocolGenerator.serverRenderUriPathParser uses
// HttpTrait.uri.segments. Trailing template slashes do not add a segment.
structure LabelInput { @required @httpLabel value: String }
structure EchoOutput { @httpPayload value: String }

@http(method: "GET", uri: "/plain/{value}")
operation Plain { input: LabelInput, output: EchoOutput }

@http(method: "GET", uri: "/trailing/{value}/")
operation Trailing { input: LabelInput, output: EchoOutput }

@http(method: "GET", uri: "/greedy/{value+}")
operation Greedy { input: LabelInput, output: EchoOutput }

@http(method: "GET", uri: "/greedy-trailing/{value+}/")
operation GreedyTrailing { input: LabelInput, output: EchoOutput }

@http(method: "GET", uri: "/suffix/{value+}/end/")
operation Suffix { input: LabelInput, output: EchoOutput }

@http(method: "GET", uri: "/query/{value}/?kind=fixed")
operation Query { input: LabelInput, output: EchoOutput }
