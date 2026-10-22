$version: "2"

// restJson1 tests for plain scalar and prefix headers: trim surrounding whitespace while
// retaining internal content, and preserve whitespace in query-map values. Response cases
// check the resulting text payload. Both legacy and schema serde run the same cases.

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test.prefixwhitespace

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

use aws.protocols#restJson1
@restJson1
service Headers { version: "1", operations: [Read] }
@http(method: "GET", uri: "/read")
operation Read {
    input := {
        @httpPrefixHeaders("X-Meta-") meta: Metadata
        @httpHeader("x-scalar") scalar: String
        @httpQueryParams query: Query
    }
    output := { @httpPayload value: String }
}
map Metadata { key: String, value: String }
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
                "a": "padded"
            },
            "scalar": "padded",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "  padded  ",
            "x-scalar": "  padded  "
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
                "a": "padded"
            },
            "scalar": "padded",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "\t padded \t",
            "x-scalar": "\t padded \t"
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
                "a": ""
            },
            "scalar": "",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "",
            "x-scalar": ""
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
                "a": ""
            },
            "scalar": "",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": " \t ",
            "x-scalar": " \t "
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader4",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "\"quoted value\""
            },
            "scalar": "\"quoted value\"",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "  \"quoted value\"  ",
            "x-scalar": "  \"quoted value\"  "
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader5",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "one,two"
            },
            "scalar": "one,two",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "  one,two  ",
            "x-scalar": "  one,two  "
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader6",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "one  two\tthree"
            },
            "scalar": "one  two\tthree",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "  one  two\tthree  ",
            "x-scalar": "  one  two\tthree  "
        },
        "queryParams": [
            "query=%20%20query%20%20"
        ]
    },
    {
        "id": "PrefixHeader7",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/read",
        "params": {
            "meta": {
                "a": "padded"
            },
            "scalar": "padded",
            "query": {
                "query": "  query  "
            }
        },
        "appliesTo": "server",
        "headers": {
            "x-meta-a": "  padded  ",
            "x-scalar": "  padded  "
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
            "value": "padded"
        },
        "body": "padded",
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
            "value": "padded"
        },
        "body": "padded",
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
            "value": ""
        },
        "body": "",
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
            "value": ""
        },
        "body": "",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload4",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "\"quoted value\""
        },
        "body": "\"quoted value\"",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload5",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "one,two"
        },
        "body": "one,two",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload6",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "one  two\tthree"
        },
        "body": "one  two\tthree",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "PrefixHeaderPayload7",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "value": "padded"
        },
        "body": "padded",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    }
])
