$version: "2"
namespace test.strictjson11
use aws.protocols#awsJson1_1
@awsJson1_1
service Payloads { version: "1", operations: [StructPayload, UnionPayload, Document, Fail] }
structure StateInput { @required state: String }
@http(uri: "/struct", method: "POST", code: 201)
operation StructPayload {
    input: StateInput
    output := { @httpPayload body: Target, @httpHeader("x-fixture") header: String, @httpResponseCode code: Integer }
}
@http(uri: "/union", method: "POST", code: 201)
operation UnionPayload { input: StateInput, output := { @httpPayload body: Choice } }
@http(uri: "/document", method: "POST", code: 201)
operation Document { input: StateInput, output: DocumentOutput }
structure DocumentOutput {
    @httpHeader("x-fixture") header: String
    @httpResponseCode code: Integer
    @jsonName("wireBody") body: Target
    choice: Choice
}
@http(uri: "/failure", method: "POST")
operation Fail { input: StateInput, output := {}, errors: [Failure] }
@error("client") @httpError(409)
structure Failure {
    @httpHeader("x-fixture") header: String
    @httpPayload body: Target
}
structure Target {
    @jsonName("wireChild") child: String
    nested: Nested
    items: Strings
    entries: Entries
}
structure Nested { value: String, flag: Boolean, number: Integer }
list Strings { member: String }
map Entries { key: String, value: String }
union Choice { text: String, nested: Target, unit: Unit }
