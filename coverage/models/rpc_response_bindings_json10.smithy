// coverage-service: coverage.responsebindings.json10#RpcResponseBindingsCoverage
// coverage-plugins: server
$version: "2"
namespace coverage.responsebindings.json10

use aws.protocols#awsJson1_0
use smithy.test#httpResponseTests

@awsJson1_0
service RpcResponseBindingsCoverage {
    version: "1"
    operations: [ResponseBindings]
}

@http(method: "PUT", uri: "/things", code: 201)
@httpResponseTests([
    {
        id: "Rpc10IgnoresOperationHttpStatusAndHeaderBinding"
        documentation: "AWS JSON returns 200 and serializes the bound member in its document without creating an ETag header."
        protocol: "aws.protocols#awsJson1_0"
        code: 200
        headers: { "Content-Type": "application/x-amz-json-1.0" }
        forbidHeaders: ["etag"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}"
        bodyMediaType: "application/json"
        params: { name: "n", etag: "\"abc\"" }
        appliesTo: "server"
    }
    {
        id: "Rpc10IgnoresMemberHttpStatusBinding"
        documentation: "The response-code member remains in the AWS JSON document and does not change the response status."
        protocol: "aws.protocols#awsJson1_0"
        code: 200
        headers: { "Content-Type": "application/x-amz-json-1.0" }
        forbidHeaders: ["etag"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"code\":299}"
        bodyMediaType: "application/json"
        params: { name: "n", etag: "\"abc\"", code: 299 }
        appliesTo: "server"
    }
    {
        id: "Rpc10IgnoresInvalidMemberHttpStatusBinding"
        documentation: "An ordinary negative integer in the AWS JSON document is not interpreted as an HTTP status."
        protocol: "aws.protocols#awsJson1_0"
        code: 200
        headers: { "Content-Type": "application/x-amz-json-1.0" }
        forbidHeaders: ["etag"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"code\":-1}"
        bodyMediaType: "application/json"
        params: { name: "n", etag: "\"abc\"", code: -1 }
        appliesTo: "server"
    }
])
operation ResponseBindings {
    input: ResponseBindingsInput
    output: ResponseBindingsOutput
}

structure ResponseBindingsInput {}

structure ResponseBindingsOutput {
    name: String
    @httpHeader("ETag")
    etag: String
    @httpResponseCode
    code: Integer
}
