// coverage-service: coverage.responsebindings.json11#RpcResponseBindingsCoverage
// coverage-plugins: server
$version: "2"
namespace coverage.responsebindings.json11

use aws.protocols#awsJson1_1
use smithy.test#httpResponseTests

@awsJson1_1
service RpcResponseBindingsCoverage {
    version: "1"
    operations: [ResponseBindings]
}

@http(method: "PUT", uri: "/things", code: 201)
@httpResponseTests([
    {
        id: "Rpc11IgnoresOperationHttpStatusAndHeaderBinding"
        documentation: "AWS JSON returns 200 and serializes the bound member in its document without creating an ETag header."
        protocol: "aws.protocols#awsJson1_1"
        code: 200
        headers: { "Content-Type": "application/x-amz-json-1.1" }
        forbidHeaders: ["etag"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}"
        bodyMediaType: "application/json"
        params: { name: "n", etag: "\"abc\"" }
        appliesTo: "server"
    }
    {
        id: "Rpc11IgnoresMemberHttpStatusBinding"
        documentation: "The response-code member remains in the AWS JSON document and does not change the response status."
        protocol: "aws.protocols#awsJson1_1"
        code: 200
        headers: { "Content-Type": "application/x-amz-json-1.1" }
        forbidHeaders: ["etag"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"code\":299}"
        bodyMediaType: "application/json"
        params: { name: "n", etag: "\"abc\"", code: 299 }
        appliesTo: "server"
    }
    {
        id: "Rpc11IgnoresInvalidMemberHttpStatusBinding"
        documentation: "An ordinary negative integer in the AWS JSON document is not interpreted as an HTTP status."
        protocol: "aws.protocols#awsJson1_1"
        code: 200
        headers: { "Content-Type": "application/x-amz-json-1.1" }
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
