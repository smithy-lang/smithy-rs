$version: "2"

// restXml request and response tests exercise timestamp formats on lists, maps, and nested
// collections, plus the protocol default on unannotated members. Both serde modes must decode
// and serialize the expected values. Kotlin checks exact response text and retains sparse-null
// cases that cannot currently pass the Smithy protocol-test parameter validator.

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test.collectiontimestamps

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

use aws.protocols#restXml
@restXml service Times { version: "1", operations: [Echo] }
@http(method: "POST", uri: "/echo")
operation Echo { input: EchoInput, output: EchoOutput }
structure EchoInput {
    dates: HttpDates, mapping: HttpMap, iso: IsoDates, epochs: EpochDates,
    nested: NestedDates, sparse: SparseDates, defaults: DefaultDates,
    nestedMapping: NestedMaps
}
structure EchoOutput {
    dates: HttpDates, mapping: HttpMap, iso: IsoDates, epochs: EpochDates,
    nested: NestedDates, sparse: SparseDates, defaults: DefaultDates,
    nestedMapping: NestedMaps
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
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "dates": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<EchoInput><dates><member>Thu, 01 Jan 1970 00:00:00 GMT</member></dates></EchoInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "TimestampsMapping",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "mapping": {
                "one": 0
            }
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<EchoInput><mapping><entry><key>one</key><value>Thu, 01 Jan 1970 00:00:00 GMT</value></entry></mapping></EchoInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "TimestampsIso",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "iso": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<EchoInput><iso><member>1970-01-01T00:00:00Z</member></iso></EchoInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "TimestampsEpochs",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "epochs": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<EchoInput><epochs><member>0</member></epochs></EchoInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "TimestampsNested",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
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
            "content-type": "application/xml"
        },
        "body": "<EchoInput><nested><member><member>Thu, 01 Jan 1970 00:00:00 GMT</member></member></nested></EchoInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "TimestampsNestedMapping",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
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
            "content-type": "application/xml"
        },
        "body": "<EchoInput><nestedMapping><entry><key>outer</key><value><entry><key>one</key><value>Thu, 01 Jan 1970 00:00:00 GMT</value></entry></value></entry></nestedMapping></EchoInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "TimestampsDefaults",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/echo",
        "params": {
            "defaults": [
                0
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<EchoInput><defaults><member>1970-01-01T00:00:00Z</member></defaults></EchoInput>",
        "bodyMediaType": "application/xml"
    }
])

apply Echo @httpResponseTests([
    {
        "id": "TimestampsDates",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "dates": [
                0
            ]
        },
        "body": "<EchoOutput><dates><member>Thu, 01 Jan 1970 00:00:00 GMT</member></dates></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsMapping",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "mapping": {
                "one": 0
            }
        },
        "body": "<EchoOutput><mapping><entry><key>one</key><value>Thu, 01 Jan 1970 00:00:00 GMT</value></entry></mapping></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsIso",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "iso": [
                0
            ]
        },
        "body": "<EchoOutput><iso><member>1970-01-01T00:00:00Z</member></iso></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsEpochs",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "epochs": [
                0
            ]
        },
        "body": "<EchoOutput><epochs><member>0</member></epochs></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsNested",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "nested": [
                [
                    0
                ]
            ]
        },
        "body": "<EchoOutput><nested><member><member>Thu, 01 Jan 1970 00:00:00 GMT</member></member></nested></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsNestedMapping",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "nestedMapping": {
                "outer": {
                    "one": 0
                }
            }
        },
        "body": "<EchoOutput><nestedMapping><entry><key>outer</key><value><entry><key>one</key><value>Thu, 01 Jan 1970 00:00:00 GMT</value></entry></value></entry></nestedMapping></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    },
    {
        "id": "TimestampsDefaults",
        "protocol": "aws.protocols#restXml",
        "code": 200,
        "params": {
            "defaults": [
                0
            ]
        },
        "body": "<EchoOutput><defaults><member>1970-01-01T00:00:00Z</member></defaults></EchoOutput>",
        "bodyMediaType": "application/xml",
        "headers": {
            "content-type": "application/xml"
        },
        "appliesTo": "server"
    }
])
