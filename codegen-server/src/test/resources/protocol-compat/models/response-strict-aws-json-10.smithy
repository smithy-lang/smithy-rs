$version: "2"

// Defines AWS JSON 1.0 output and error scenarios with documents, structures, unions, text,
// blobs, and REST-style bindings. Named handler modes populate or omit values so strict
// compatibility tests can compare complete legacy and schema HTTP responses.

namespace test.strictawsjson10
use aws.protocols#awsJson1_0
@awsJson1_0
service StrictAwsJson10 {
    version: "1"
    operations: [DocumentResponse, StructResponse, UnionResponse, TextResponse, BlobResponse]
}
structure ModeInput { @httpQuery("case") mode: String }
@http(method: "GET", uri: "/document", code: 201)
operation DocumentResponse { input: ModeInput, output: DocumentOutput, errors: [Failure] }
structure DocumentOutput {
    @jsonName("wireRecord") record: Record
    choice: Choice
    @httpResponseCode code: Integer
    @httpHeader("x-value") header: String
    @httpHeader("x-tags") tags: Strings
    @httpPrefixHeaders("x-meta-") metadata: StringMap
    @httpHeader("content-type") media: String
}
@http(method: "GET", uri: "/struct", code: 202)
operation StructResponse { input: ModeInput, output := {
    @httpPayload body: Record
    @httpHeader("x-value") header: String
}, errors: [PayloadFailure] }
@http(method: "GET", uri: "/union", code: 200)
operation UnionResponse { input: ModeInput, output := { @httpPayload body: Choice }, errors: [UnionFailure] }
@http(method: "GET", uri: "/text", code: 200)
operation TextResponse { input: ModeInput, output := { @httpPayload body: String } }
@http(method: "GET", uri: "/blob", code: 200)
operation BlobResponse { input: ModeInput, output := { @httpPayload body: Blob } }
structure Record {
    @jsonName("wireChild") child: String
    items: Strings
    entries: StringMap
    nested: Nested
}
structure Nested { enabled: Boolean, number: Integer }
union Choice { text: String, record: Record, items: Strings }
list Strings { member: String }
map StringMap { key: String, value: String }
@error("client") @httpError(409)
structure Failure {
    message: String
    record: Record
    @httpHeader("x-error") header: String
    @httpPrefixHeaders("x-error-meta-") metadata: StringMap
}
@error("client") @httpError(422)
structure UnionFailure { @httpPayload body: Choice }
@error("server") @httpError(503)
structure PayloadFailure {
    @httpPayload body: Record
    @httpHeader("x-error") header: String
}
