// coverage-service: coverage.rpcbindings.cbor#RpcResponseBindings
// coverage-plugins: server
// coverage-codegen: "http-1x": true
$version: "2"
namespace coverage.rpcbindings.cbor
use smithy.protocols#rpcv2Cbor
use smithy.test#httpResponseTests

@rpcv2Cbor
service RpcResponseBindings {
    version: "1"
    operations: [Respond]
}

@http(method: "PUT", uri: "/things", code: 201)
@httpResponseTests([
    {
        id: "RpcCborIgnoresRestResponseBindingsWithoutCode"
        protocol: "smithy.protocols#rpcv2Cbor"
        appliesTo: "server"
        code: 200
        headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" }
        forbidHeaders: ["etag"]
        body: "omRuYW1lYW5kZXRhZ2UiYWJjIg=="
        bodyMediaType: "application/cbor"
        params: { name: "n", etag: "\"abc\"" }
    }
    {
        id: "RpcCborIgnoresRestResponseBindingsWithCode"
        protocol: "smithy.protocols#rpcv2Cbor"
        appliesTo: "server"
        code: 200
        headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" }
        forbidHeaders: ["etag"]
        body: "o2RuYW1lYW5kZXRhZ2UiYWJjImRjb2RlGQEr"
        bodyMediaType: "application/cbor"
        params: { name: "n", etag: "\"abc\"", code: 299 }
    }
    {
        id: "RpcCborNegativeCodeIsDocumentData"
        protocol: "smithy.protocols#rpcv2Cbor"
        appliesTo: "server"
        code: 200
        headers: { "Content-Type": "application/cbor", "smithy-protocol": "rpc-v2-cbor" }
        forbidHeaders: ["etag"]
        body: "o2RuYW1lYW5kZXRhZ2UiYWJjImRjb2RlIA=="
        bodyMediaType: "application/cbor"
        params: { name: "n", etag: "\"abc\"", code: -1 }
    }
])
operation Respond {
    output: BindingOutput
}

structure BindingOutput {
    name: String
    @httpHeader("ETag")
    etag: String
    @httpResponseCode
    code: Integer
}
