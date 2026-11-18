$version: "2"

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test

use smithy.protocols#rpcv2Cbor
use smithy.test#httpResponseTests

// Observed from fresh generated legacy serde before migrating the TSV snapshots.
// RPC v2 CBOR ignores the REST-style status and header bindings: status stays 200,
// ETag is absent, and all output members remain in the CBOR body.
// These server-only expectations run with generated serde and default schema serde.
// Content-Length must match the emitted body; CBOR is compared semantically.
// Body fixtures are base64 encodings of the original observed CBOR bytes.

@rpcv2Cbor
service Example {
    version: "1"
    operations: [Write]
}

@http(method: "PUT", uri: "/things/{name}", code: 201)
@httpResponseTests([
    {
        id: "LegacyRpcV2CborResponseCodeAbsent"
        documentation: "Legacy output code=None: CBOR ignores @http status 201 and does not emit the bound ETag."
        protocol: "smithy.protocols#rpcv2Cbor"
        appliesTo: "server"
        code: 200
        params: {name: "n", etag: "\"abc\""}
        headers: {
            "content-type": "application/cbor"
            "smithy-protocol": "rpc-v2-cbor"
        }
        requireHeaders: ["content-length"]
        forbidHeaders: ["etag"]
        body: "v2RuYW1lYW5kZXRhZ2UiYWJjIv8="
        bodyMediaType: "application/cbor"
        vendorParamsShape: "smithy.rust.test#ResponseTestOptions"
        vendorParams: {forbidAdditionalHeaders: true}
    }
    {
        id: "LegacyRpcV2CborResponseCode299"
        documentation: "Legacy output code=Some(299): the response-code member stays in the body and does not override status 200."
        protocol: "smithy.protocols#rpcv2Cbor"
        appliesTo: "server"
        code: 200
        params: {name: "n", etag: "\"abc\"", code: 299}
        headers: {
            "content-type": "application/cbor"
            "smithy-protocol": "rpc-v2-cbor"
        }
        requireHeaders: ["content-length"]
        forbidHeaders: ["etag"]
        body: "v2RuYW1lYW5kZXRhZ2UiYWJjImRjb2RlGQEr/w=="
        bodyMediaType: "application/cbor"
        vendorParamsShape: "smithy.rust.test#ResponseTestOptions"
        vendorParams: {forbidAdditionalHeaders: true}
    }
    {
        id: "LegacyRpcV2CborResponseCodeNegative"
        documentation: "Legacy output code=Some(-1): a negative response-code member remains ordinary body data."
        protocol: "smithy.protocols#rpcv2Cbor"
        appliesTo: "server"
        code: 200
        params: {name: "n", etag: "\"abc\"", code: -1}
        headers: {
            "content-type": "application/cbor"
            "smithy-protocol": "rpc-v2-cbor"
        }
        requireHeaders: ["content-length"]
        forbidHeaders: ["etag"]
        body: "v2RuYW1lYW5kZXRhZ2UiYWJjImRjb2RlIP8="
        bodyMediaType: "application/cbor"
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
structure Failure {
    message: String
    @httpHeader("ETag")
    etag: String
    code: Integer
}
