$version: "2"

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test

use aws.protocols#awsJson1_1
use smithy.test#httpResponseTests

// Observed from fresh generated legacy serde before migrating the TSV snapshots.
// Historical AWS JSON honors @http and @httpError status codes and emits ETag,
// while retaining header-bound members and @httpResponseCode members in the body.
// These server-only expectations run with generated serde and default schema serde.
// Content-Length must be present and match the emitted body; JSON is compared semantically.

@awsJson1_1
service Example {
    version: "1"
    operations: [Write]
}

@http(method: "PUT", uri: "/things/{name}", code: 201)
@httpResponseTests([
    {
        id: "LegacyAwsJson11ResponseCodeAbsent"
        documentation: "Legacy output code=None: historical AWS JSON returns the modeled HTTP status and ETag."
        protocol: "aws.protocols#awsJson1_1"
        appliesTo: "server"
        code: 201
        params: {name: "n", etag: "\"abc\""}
        headers: {
            etag: "\"abc\""
            "content-type": "application/x-amz-json-1.1"
        }
        requireHeaders: ["content-length"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}"
        bodyMediaType: "application/json"
        vendorParamsShape: "smithy.rust.test#ResponseTestOptions"
        vendorParams: {forbidAdditionalHeaders: true}
    }
    {
        id: "LegacyAwsJson11ResponseCode299"
        documentation: "Legacy output code=Some(299): the response-code member stays in the body and does not override status 201."
        protocol: "aws.protocols#awsJson1_1"
        appliesTo: "server"
        code: 201
        params: {name: "n", etag: "\"abc\"", code: 299}
        headers: {
            etag: "\"abc\""
            "content-type": "application/x-amz-json-1.1"
        }
        requireHeaders: ["content-length"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"code\":299}"
        bodyMediaType: "application/json"
        vendorParamsShape: "smithy.rust.test#ResponseTestOptions"
        vendorParams: {forbidAdditionalHeaders: true}
    }
    {
        id: "LegacyAwsJson11ResponseCodeNegative"
        documentation: "Legacy output code=Some(-1): a negative response-code member remains ordinary body data."
        protocol: "aws.protocols#awsJson1_1"
        appliesTo: "server"
        code: 201
        params: {name: "n", etag: "\"abc\"", code: -1}
        headers: {
            etag: "\"abc\""
            "content-type": "application/x-amz-json-1.1"
        }
        requireHeaders: ["content-length"]
        body: "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"code\":-1}"
        bodyMediaType: "application/json"
        vendorParamsShape: "smithy.rust.test#ResponseTestOptions"
        vendorParams: {forbidAdditionalHeaders: true}
    }
])
operation Write {
    input := {
        @required
        @httpLabel
        name: String
    }
    output: Out
    errors: [Failure]
}

structure Out {
    name: String
    @httpHeader("ETag")
    etag: String
    @httpResponseCode
    code: Integer
}

@error("client")
@httpError(422)
@httpResponseTests([
    {
        id: "LegacyAwsJson11ErrorResponseBindings"
        documentation: "Legacy error: @httpError and ETag are honored while the body retains ETag and the unqualified error type."
        protocol: "aws.protocols#awsJson1_1"
        appliesTo: "server"
        code: 422
        params: {message: "n", etag: "\"abc\"", code: 299}
        headers: {
            etag: "\"abc\""
            "content-type": "application/x-amz-json-1.1"
        }
        requireHeaders: ["content-length"]
        body: "{\"message\":\"n\",\"etag\":\"\\\"abc\\\"\",\"code\":299,\"__type\":\"Failure\"}"
        bodyMediaType: "application/json"
        vendorParamsShape: "smithy.rust.test#ResponseTestOptions"
        vendorParams: {forbidAdditionalHeaders: true}
    }
])
structure Failure {
    message: String
    @httpHeader("ETag")
    etag: String
    code: Integer
}
