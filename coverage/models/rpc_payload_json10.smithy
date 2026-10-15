// coverage-service: coverage.payload.json10#RpcPayloadCoverage
// coverage-plugins: server
$version: "2"
namespace coverage.payload.json10
use aws.protocols#awsJson1_0
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@awsJson1_0
service RpcPayloadCoverage { version: "1", operations: [Structured, UnionPayload] }

@http(method: "POST", uri: "/payload", code: 201)
@httpRequestTests([{
    id: "Rpc10StructuredPayloadRequest", protocol: "aws.protocols#awsJson1_0",
    method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.0",
        "X-Amz-Target": "RpcPayloadCoverage.Structured" },
    body: "{\"body\":{\"child\":\"value\"}}", bodyMediaType: "application/json",
    params: { body: { child: "value" } }, appliesTo: "server"
}])
@httpResponseTests([{
    id: "Rpc10StructuredPayloadResponse", protocol: "aws.protocols#awsJson1_0",
    code: 200, headers: { "Content-Type": "application/x-amz-json-1.0" },
    body: "{\"body\":{\"child\":\"value\"}}", bodyMediaType: "application/json",
    params: { body: { child: "value" } }, appliesTo: "server"
}])
operation Structured { input: StructuredInput, output: StructuredOutput }
structure StructuredInput { @httpPayload body: Target }
structure StructuredOutput { @httpPayload body: Target }
@xmlName("TargetRoot")
structure Target { child: String }

@http(method: "POST", uri: "/union", code: 201)
@httpRequestTests([{
    id: "Rpc10UnionPayloadRequest", protocol: "aws.protocols#awsJson1_0",
    method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.0",
        "X-Amz-Target": "RpcPayloadCoverage.UnionPayload" },
    body: "{\"body\":{\"text\":\"value\"}}", bodyMediaType: "application/json",
    params: { body: { text: "value" } }, appliesTo: "server"
}])
@httpResponseTests([{
    id: "Rpc10UnionPayloadResponse", protocol: "aws.protocols#awsJson1_0",
    code: 200, headers: { "Content-Type": "application/x-amz-json-1.0" },
    body: "{\"body\":{\"text\":\"value\"}}", bodyMediaType: "application/json",
    params: { body: { text: "value" } }, appliesTo: "server"
}])
operation UnionPayload { input: UnionInput, output: UnionOutput }
structure UnionInput { @httpPayload body: Choice }
structure UnionOutput { @httpPayload body: Choice }
@xmlName("ChoiceRoot")
union Choice { text: String, record: Target }
