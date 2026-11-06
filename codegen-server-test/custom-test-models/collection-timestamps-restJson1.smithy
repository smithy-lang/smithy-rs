$version: "2"

// restJson1 request and response tests exercise timestamp formats on lists, maps, and nested
// collections, plus the protocol default on unannotated members. Both serde modes must decode
// and serialize the expected values. Kotlin checks exact response text and retains sparse-null
// cases that cannot currently pass the Smithy protocol-test parameter validator.

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test.collectiontimestamps

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

use aws.protocols#restJson1
@restJson1 service Times { version: "1", operations: [Echo] }
@http(method: "POST", uri: "/echo")
operation Echo { input: EchoInput, output: EchoOutput }
structure EchoInput {
    dates: HttpDates, mapping: HttpMap, iso: IsoDates, epochs: EpochDates,
    nested: NestedDates, sparse: SparseDates, defaults: DefaultDates,
    nestedMapping: NestedMaps, sparseMapping: SparseMap
}
structure EchoOutput {
    dates: HttpDates, mapping: HttpMap, iso: IsoDates, epochs: EpochDates,
    nested: NestedDates, sparse: SparseDates, defaults: DefaultDates,
    nestedMapping: NestedMaps, sparseMapping: SparseMap
}
list HttpDates { @timestampFormat("http-date") member: Timestamp }
map HttpMap { key: String, @timestampFormat("http-date") value: Timestamp }
list IsoDates { @timestampFormat("date-time") member: Timestamp }
list EpochDates { @timestampFormat("epoch-seconds") member: Timestamp }
list NestedDates { member: HttpDates }
@sparse list SparseDates { @timestampFormat("http-date") member: Timestamp }
list DefaultDates { member: Timestamp }
map NestedMaps { key: String, value: HttpMap }
@sparse map SparseMap { key: String, @timestampFormat("http-date") value: Timestamp }

apply Echo @httpRequestTests([
    {
        "id": "TimestampsDates",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "dates": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"dates\":[\"Thu, 01 Jan 1970 00:00:00 GMT\"]}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsMapping",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "mapping": {
                "one": 0
            }
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"mapping\":{\"one\":\"Thu, 01 Jan 1970 00:00:00 GMT\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsIso",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "iso": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"iso\":[\"1970-01-01T00:00:00Z\"]}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsEpochs",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "epochs": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"epochs\":[0]}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsNested",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "nested": [
                [
                    0
                ]
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"nested\":[[\"Thu, 01 Jan 1970 00:00:00 GMT\"]]}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsNestedMapping",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "nestedMapping": {
                "outer": {
                    "one": 0
                }
            }
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"nestedMapping\":{\"outer\":{\"one\":\"Thu, 01 Jan 1970 00:00:00 GMT\"}}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsSparseMappingValue",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "sparseMapping": {
                "one": 0
            }
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"sparseMapping\":{\"one\":\"Thu, 01 Jan 1970 00:00:00 GMT\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "TimestampsDefaults",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "defaults": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"defaults\":[0]}",
        "bodyMediaType": "application/json"
    }
])

apply Echo @httpResponseTests([
    {
        "id": "TimestampsDates",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "dates": [
                0
            ]
        },
        "body": "{\"dates\":[\"Thu, 01 Jan 1970 00:00:00 GMT\"]}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsMapping",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "mapping": {
                "one": 0
            }
        },
        "body": "{\"mapping\":{\"one\":\"Thu, 01 Jan 1970 00:00:00 GMT\"}}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsIso",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "iso": [
                0
            ]
        },
        "body": "{\"iso\":[\"1970-01-01T00:00:00Z\"]}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsEpochs",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "epochs": [
                0
            ]
        },
        "body": "{\"epochs\":[0]}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsNested",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "nested": [
                [
                    0
                ]
            ]
        },
        "body": "{\"nested\":[[\"Thu, 01 Jan 1970 00:00:00 GMT\"]]}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsNestedMapping",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "nestedMapping": {
                "outer": {
                    "one": 0
                }
            }
        },
        "body": "{\"nestedMapping\":{\"outer\":{\"one\":\"Thu, 01 Jan 1970 00:00:00 GMT\"}}}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsSparseMappingValue",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "sparseMapping": {
                "one": 0
            }
        },
        "body": "{\"sparseMapping\":{\"one\":\"Thu, 01 Jan 1970 00:00:00 GMT\"}}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsDefaults",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "defaults": [
                0
            ]
        },
        "body": "{\"defaults\":[0]}",
        "bodyMediaType": "application/json",
        "headers": {
            "content-type": "application/json"
        },
        "appliesTo": "server"
    }
])
