$version: "2"
namespace compat.errorcontenttype
use aws.protocols#restJson1
use aws.protocols#restXml
use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use smithy.protocols#rpcv2Cbor
@restJson1
service JsonErrorContentType { version: "1", operations: [Empty, Document, Text, MediaText, RawBlob, MediaBlob, Stream] }
@restXml
service XmlErrorContentType { version: "1", operations: [Empty, Document, Text, MediaText, RawBlob, MediaBlob, Stream] }

@http(method: "GET", uri: "/Empty")
operation Empty { output := {  }, errors: [Failure] }

@http(method: "GET", uri: "/Document")
operation Document { output := { value: String }, errors: [Failure] }

@http(method: "GET", uri: "/Text")
operation Text { output := { @httpPayload body: String }, errors: [Failure] }

@http(method: "GET", uri: "/MediaText")
operation MediaText { output := { @httpPayload body: MediaString }, errors: [Failure] }

@http(method: "GET", uri: "/RawBlob")
operation RawBlob { output := { @httpPayload body: smithy.api#Blob }, errors: [Failure] }

@http(method: "GET", uri: "/MediaBlob")
operation MediaBlob { output := { @httpPayload body: MediaBytes }, errors: [Failure] }

@http(method: "GET", uri: "/Stream")
operation Stream { output := { @required @httpPayload body: StreamingBody }, errors: [Failure] }

@error("client")
@httpError(422)
structure Failure {
    message: String
    @httpHeader("content-type") contentType: String
}
@mediaType("application/custom-text")
string MediaString
@mediaType("application/custom-binary")
blob MediaBytes
@streaming
blob StreamingBody

@awsJson1_0
service Json10ErrorContentType { version: "1", operations: [Empty, Document, Text, MediaText, RawBlob, MediaBlob] }
@awsJson1_1
service Json11ErrorContentType { version: "1", operations: [Empty, Document, Text, MediaText, RawBlob, MediaBlob] }
@rpcv2Cbor
service CborErrorContentType { version: "1", operations: [Empty, Document, Text, MediaText, RawBlob, MediaBlob] }
