// coverage-service: coverage.payload.cbor#RpcPayloadCoverage
// coverage-plugins: server
// coverage-codegen: "http-1x": true
$version: "2"
namespace coverage.payload.cbor
use smithy.protocols#rpcv2Cbor
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@rpcv2Cbor
service RpcPayloadCoverage { version: "1", operations: [Structured, UnionPayload] }

@http(method: "POST", uri: "/payload", code: 201)
@httpRequestTests([{
    id: "RpcCborStructuredPayloadRequest", protocol: "smithy.protocols#rpcv2Cbor",
    method: "POST", uri: "/service/RpcPayloadCoverage/operation/Structured",
    headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" },
    body: "oWRib2R5oWVjaGlsZGV2YWx1ZQ==", bodyMediaType: "application/cbor",
    params: { body: { child: "value" } }, appliesTo: "server"
}])
@httpResponseTests([{
    id: "RpcCborStructuredPayloadResponse", protocol: "smithy.protocols#rpcv2Cbor",
    code: 200, headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" },
    body: "oWRib2R5oWVjaGlsZGV2YWx1ZQ==", bodyMediaType: "application/cbor",
    params: { body: { child: "value" } }, appliesTo: "server"
}])
operation Structured { input: StructuredInput, output: StructuredOutput }
structure StructuredInput { @httpPayload body: Target }
structure StructuredOutput { @httpPayload body: Target }
@xmlName("TargetRoot")
structure Target { child: String }

@http(method: "POST", uri: "/union", code: 201)
@httpRequestTests([{
    id: "RpcCborUnionPayloadRequest", protocol: "smithy.protocols#rpcv2Cbor",
    method: "POST", uri: "/service/RpcPayloadCoverage/operation/UnionPayload",
    headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" },
    body: "oWRib2R5oWR0ZXh0ZXZhbHVl", bodyMediaType: "application/cbor",
    params: { body: { text: "value" } }, appliesTo: "server"
}])
@httpResponseTests([{
    id: "RpcCborUnionPayloadResponse", protocol: "smithy.protocols#rpcv2Cbor",
    code: 200, headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" },
    body: "oWRib2R5oWR0ZXh0ZXZhbHVl", bodyMediaType: "application/cbor",
    params: { body: { text: "value" } }, appliesTo: "server"
}])
operation UnionPayload { input: UnionInput, output: UnionOutput }
structure UnionInput { @httpPayload body: Choice }
structure UnionOutput { @httpPayload body: Choice }
@xmlName("ChoiceRoot")
union Choice { text: String, record: Target }
