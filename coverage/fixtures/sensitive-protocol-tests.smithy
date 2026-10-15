$version: "2"
namespace coverage.fixtures.sensitive

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.sensitive#GetSecret @smithy.test#httpRequestTests([
    {
        "id": "CoverageSensitiveGetRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/secret/secret-7",
        "headers": {
            "x-auth": "auth-secret",
            "x-meta-origin": "private"
        },
        "body": "",
        "bodyMediaType": "application/json",
        "params": {
            "secretId": "secret-7",
            "token": "token-9",
            "authHeader": "auth-secret",
            "meta": {
                "origin": "private"
            }
        },
        "appliesTo": "server",
        "queryParams": [
            "token=token-9"
        ]
    }
])

apply test.sensitive#GetSecret @smithy.test#httpResponseTests([
    {
        "id": "CoverageSensitiveStructuredPayloadResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json",
            "x-secret-value": "header-secret"
        },
        "body": "{\"name\":\"key\",\"value\":\"secret\"}",
        "bodyMediaType": "application/json",
        "params": {
            "value": "header-secret",
            "body": {
                "name": "key",
                "value": "secret"
            }
        },
        "appliesTo": "server"
    }
])

apply test.sensitive#PutSecret @smithy.test#httpRequestTests([
    {
        "id": "CoverageSensitiveBlobRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/secret",
        "headers": {
            "content-type": "application/octet-stream"
        },
        "body": "secretblob",
        "bodyMediaType": "application/octet-stream",
        "params": {
            "body": "secretblob"
        },
        "appliesTo": "server"
    }
])

apply test.sensitive#PutSecret @smithy.test#httpResponseTests([
    {
        "id": "CoverageSensitiveNestedResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"secret\":\"secret-value\",\"nested\":{\"inner\":\"private\",\"plain\":\"public\"}}",
        "bodyMediaType": "application/json",
        "params": {
            "code": 201,
            "secret": "secret-value",
            "nested": {
                "inner": "private",
                "plain": "public"
            }
        },
        "appliesTo": "server"
    }
])

apply test.sensitive#ProxySecret @smithy.test#httpRequestTests([
    {
        "id": "CoverageSensitiveGreedyLabelRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/proxy/alpha/beta/end",
        "headers": {},
        "body": "",
        "bodyMediaType": "application/json",
        "params": {
            "path": "alpha/beta"
        },
        "appliesTo": "server"
    }
])

apply test.sensitive#ProxySecret @smithy.test#httpResponseTests([
    {
        "id": "CoverageSensitiveStatusResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 206,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{}",
        "bodyMediaType": "application/json",
        "params": {
            "code": 206
        },
        "appliesTo": "server"
    }
])

apply test.sensitive#QuerySecret @smithy.test#httpRequestTests([
    {
        "id": "CoverageSensitiveQueryAndPrefixRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/query-secret",
        "headers": {
            "x-key-secretkey": "plainvalue"
        },
        "body": "",
        "bodyMediaType": "application/json",
        "params": {
            "params": {
                "first": "private",
                "second": "other"
            },
            "keyed": {
                "secretkey": "plainvalue"
            }
        },
        "appliesTo": "server",
        "queryParams": [
            "first=private",
            "second=other"
        ]
    }
])

apply test.sensitive#QuerySecret @smithy.test#httpResponseTests([
    {
        "id": "CoverageSensitiveQueryResponse",
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
