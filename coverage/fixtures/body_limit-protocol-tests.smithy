$version: "2"
namespace coverage.fixtures.body_limit

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.bodylimit#PutDocumentBody @smithy.test#httpRequestTests([
    {
        "id": "CoverageBodyLimitPutDocumentBodyRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/doc",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"name\":\"body-name\",\"count\":17}",
        "bodyMediaType": "application/json",
        "params": {
            "name": "body-name",
            "count": 17
        },
        "appliesTo": "server"
    }
])

apply test.bodylimit#PutDocumentBody @smithy.test#httpResponseTests([
    {
        "id": "CoverageBodyLimitPutDocumentBodyResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"ok\":true}",
        "bodyMediaType": "application/json",
        "params": {
            "ok": true
        },
        "appliesTo": "server"
    }
])

apply test.bodylimit#PutBlobPayload @smithy.test#httpRequestTests([
    {
        "id": "CoverageBodyLimitPutBlobPayloadRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/blob",
        "headers": {
            "content-type": "application/octet-stream"
        },
        "body": "blob",
        "bodyMediaType": "application/octet-stream",
        "params": {
            "data": "blob"
        },
        "appliesTo": "server"
    }
])

apply test.bodylimit#PutBlobPayload @smithy.test#httpResponseTests([
    {
        "id": "CoverageBodyLimitPutBlobPayloadResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"ok\":true}",
        "bodyMediaType": "application/json",
        "params": {
            "ok": true
        },
        "appliesTo": "server"
    }
])

apply test.bodylimit#PutStringPayload @smithy.test#httpRequestTests([
    {
        "id": "CoverageBodyLimitPutStringPayloadRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/text",
        "headers": {
            "content-type": "text/plain"
        },
        "body": "populated text",
        "bodyMediaType": "text/plain",
        "params": {
            "text": "populated text"
        },
        "appliesTo": "server"
    }
])

apply test.bodylimit#PutStringPayload @smithy.test#httpResponseTests([
    {
        "id": "CoverageBodyLimitPutStringPayloadResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"ok\":true}",
        "bodyMediaType": "application/json",
        "params": {
            "ok": true
        },
        "appliesTo": "server"
    }
])
