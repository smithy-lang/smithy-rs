$version: "2"
namespace compat.responseprefixmedia

use aws.protocols#restJson1
use aws.protocols#restXml

@restJson1
service JsonResponsePrefixMedia { version: "1", operations: [Emit, Fail, Stream] }
@restXml
service XmlResponsePrefixMedia { version: "1", operations: [Emit, Fail, Stream] }

@http(method: "GET", uri: "/emit")
operation Emit {
    output := {
        @httpPrefixHeaders("X-Media-") meta: MediaMap
        @httpHeader("x-direct") direct: MediaString
        @httpHeader("x-plain") plain: String
        @httpPayload body: String
    }
}
@http(method: "GET", uri: "/fail")
operation Fail { output := { value: String }, errors: [Failure] }
@error("client")
@httpError(422)
structure Failure {
    message: String
    @httpPrefixHeaders("X-Media-") meta: MediaMap
    @httpHeader("x-direct") direct: MediaString
    @httpHeader("x-plain") plain: String
}
@http(method: "GET", uri: "/stream")
operation Stream {
    output := {
        @httpPrefixHeaders("X-Media-") meta: MediaMap
        @httpHeader("x-direct") direct: MediaString
        @httpHeader("x-plain") plain: String
        @required @httpPayload body: StreamingBody
    }
}
@streaming
blob StreamingBody
@mediaType("application/json")
string MediaString
map MediaMap { key: String, value: MediaString }
