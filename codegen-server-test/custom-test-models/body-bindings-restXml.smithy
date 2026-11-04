$version: "2"

// restXml request tests put copies of URI-label, header, and query members into the body,
// including invalidly typed copies. Those values must not replace the transport-bound inputs.
// Response tests check the handler-result payload. Both serde modes run these cases; Kotlin
// comparisons additionally pin complete response headers and exact body bytes.

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test.bodybindings

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

use aws.protocols#restXml
use smithy.framework#ValidationException

@restXml
service Orders { version: "1", operations: [CreateOrder] }

@http(method: "POST", uri: "/orders/{storeId}", code: 201)
operation CreateOrder {
    input: CreateOrderInput
    output := { @httpPayload value: String }
    errors: [ValidationException]
}

@xmlName("OrderDocument")
structure CreateOrderInput {
    @required @httpLabel storeId: String
    @httpHeader("x-tenant-id") tenantId: String
    @httpQuery("dryRun") dryRun: Boolean
    note: String
    quantity: Integer
    detail: Detail
}

structure Detail { tenantId: String }

apply CreateOrder @httpRequestTests([
    {
        "id": "BodyTransportMembers0WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            },
            "tenantId": "tenant-A"
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml",
            "x-tenant-id": "tenant-A"
        },
        "body": "<OrderDocument><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers0WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            }
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<OrderDocument><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers1WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            },
            "tenantId": "tenant-A"
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml",
            "x-tenant-id": "tenant-A"
        },
        "body": "<OrderDocument><storeId>evil</storeId><tenantId>tenant-B</tenantId><dryRun>true</dryRun><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers1WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            }
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<OrderDocument><storeId>evil</storeId><tenantId>tenant-B</tenantId><dryRun>true</dryRun><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers2WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            },
            "tenantId": "tenant-A"
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml",
            "x-tenant-id": "tenant-A"
        },
        "body": "<OrderDocument><storeId><ignored/></storeId><tenantId><ignored/></tenantId><dryRun>yes-please</dryRun><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers2WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            }
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<OrderDocument><storeId><ignored/></storeId><tenantId><ignored/></tenantId><dryRun>yes-please</dryRun><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers3WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            },
            "tenantId": "tenant-A"
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml",
            "x-tenant-id": "tenant-A"
        },
        "body": "<OrderDocument><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail><tenantId><note>decoy</note></tenantId><dryRun><ignored/></dryRun></OrderDocument>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "BodyTransportMembers3WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/orders/store-7",
        "params": {
            "storeId": "store-7",
            "dryRun": false,
            "note": "hi",
            "quantity": 2,
            "detail": {
                "tenantId": "nested"
            }
        },
        "appliesTo": "server",
        "queryParams": [
            "dryRun=false"
        ],
        "headers": {
            "content-type": "application/xml"
        },
        "body": "<OrderDocument><note>hi</note><quantity>2</quantity><detail><tenantId>nested</tenantId></detail><tenantId><note>decoy</note></tenantId><dryRun><ignored/></dryRun></OrderDocument>",
        "bodyMediaType": "application/xml"
    }
])

apply CreateOrder @httpResponseTests([
    {
        "id": "BodyPayloadtenantA",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "params": {
            "value": "store-7|tenant-A|false|hi|2|nested"
        },
        "body": "store-7|tenant-A|false|hi|2|nested",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    },
    {
        "id": "BodyPayloadunset",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "params": {
            "value": "store-7|unset|false|hi|2|nested"
        },
        "body": "store-7|unset|false|hi|2|nested",
        "bodyMediaType": "text/plain",
        "headers": {
            "content-type": "text/plain"
        },
        "appliesTo": "server"
    }
])
