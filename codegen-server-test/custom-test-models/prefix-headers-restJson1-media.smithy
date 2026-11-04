$version: "2"

// restJson1 tests for media-typed scalar and prefix headers: decode base64 and reject malformed
// encodings or invalid UTF-8 before calling the handler. Response cases check the decoded text.
// Both serde modes run these cases; repeated-header rejection remains in the Kotlin/Rust suite
// because protocol-test traits represent only one value per header name.

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test.prefixwhitespace

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use smithy.test#httpMalformedRequestTests

use aws.protocols#restJson1
@restJson1
service Headers { version: "1", operations: [Read] }
@http(method: "GET", uri: "/read")
operation Read {
    input := {
        @httpPrefixHeaders("X-Meta-") meta: Metadata
        @httpHeader("x-scalar") scalar: JsonString
        @httpQueryParams query: Query
    }
    output := { @httpPayload value: String }
}
map Metadata { key: String, value: JsonString }
map Query { key: String, value: String }
@mediaType("application/json")
string JsonString

apply Read @httpRequestTests([
    {
        "id": "PrefixHeader0",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "true"
            },
            "scalar": "true",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "dHJ1ZQ==",
            "x-scalar": "dHJ1ZQ=="
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader1",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "true"
            },
            "scalar": "true",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "  dHJ1ZQ==  ",
            "x-scalar": "  dHJ1ZQ==  "
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader2",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "true"
            },
            "scalar": "true",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "\"dHJ1ZQ==\"",
            "x-scalar": "\"dHJ1ZQ==\""
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader3",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "héllo"
            },
            "scalar": "héllo",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "aMOpbGxv",
            "x-scalar": "aMOpbGxv"
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    }
])

apply Read @httpResponseTests([
    {
        "id": "PrefixHeaderPayload0",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "true"
        },
        "body": "true",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload1",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "true"
        },
        "body": "true",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload2",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "true"
        },
        "body": "true",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload3",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "héllo"
        },
        "body": "héllo",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    }
])

apply Read @httpMalformedRequestTests([
    {
        "id": "InvalidMediaPrefixHeader0",
        "protocol": "aws.protocols#restJson1",
        "documentation": "Invalid encoded prefix headers are rejected before the handler runs.",
        "request": {
            "method": "GET",
            "uri": "/read",
            "headers": {
                "x-meta-a": "%%%"
            }
        },
        "response": {
            "code": 400,
            "headers": {
                "content-type": "application/json",
                "x-amzn-errortype": "SerializationException"
            },
            "body": {
                "mediaType": "text/plain",
                "assertion": {
                    "contents": "{}"
                }
            }
        }
    },
    {
        "id": "InvalidMediaPrefixHeader1",
        "protocol": "aws.protocols#restJson1",
        "documentation": "Invalid encoded prefix headers are rejected before the handler runs.",
        "request": {
            "method": "GET",
            "uri": "/read",
            "headers": {
                "x-meta-a": "/w=="
            }
        },
        "response": {
            "code": 400,
            "headers": {
                "content-type": "application/json",
                "x-amzn-errortype": "SerializationException"
            },
            "body": {
                "mediaType": "text/plain",
                "assertion": {
                    "contents": "{}"
                }
            }
        }
    },
    {
        "id": "InvalidMediaPrefixHeader2",
        "protocol": "aws.protocols#restJson1",
        "documentation": "Invalid encoded prefix headers are rejected before the handler runs.",
        "request": {
            "method": "GET",
            "uri": "/read",
            "headers": {
                "x-meta-a": "dHJ1ZQ==,dHJ1ZQ=="
            }
        },
        "response": {
            "code": 400,
            "headers": {
                "content-type": "application/json",
                "x-amzn-errortype": "SerializationException"
            },
            "body": {
                "mediaType": "text/plain",
                "assertion": {
                    "contents": "{}"
                }
            }
        }
    }
])
