$version: "2"

// restJson1 request tests put copies of URI-label, header, and query members into the body,
// including invalidly typed copies. Those values must not replace the transport-bound inputs.
// Response tests check the handler-result payload. Both serde modes run these cases; Kotlin
// comparisons additionally pin complete response headers and exact body bytes.

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace test.bodybindings

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

use aws.protocols#restJson1
use smithy.framework#ValidationException

@restJson1
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
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json",
            "x-tenant-id": "tenant-A"
        },
        "body": "{\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers0WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json"
        },
        "body": "{\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers1WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json",
            "x-tenant-id": "tenant-A"
        },
        "body": "{\"storeId\":\"evil\",\"tenantId\":\"tenant-B\",\"dryRun\":true,\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers1WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json"
        },
        "body": "{\"storeId\":\"evil\",\"tenantId\":\"tenant-B\",\"dryRun\":true,\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers2WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json",
            "x-tenant-id": "tenant-A"
        },
        "body": "{\"storeId\":[],\"tenantId\":{},\"dryRun\":\"yes-please\",\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers2WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json"
        },
        "body": "{\"storeId\":[],\"tenantId\":{},\"dryRun\":\"yes-please\",\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers3WithHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json",
            "x-tenant-id": "tenant-A"
        },
        "body": "{\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"},\"tenantId\":[{\"note\":\"decoy\"}],\"dryRun\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "BodyTransportMembers3WithoutHeader",
        "documentation": "Preserves observed generated serde behavior; exercised with schema serde enabled and disabled.",
        "protocol": "aws.protocols#restJson1",
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
            "content-type": "application/json"
        },
        "body": "{\"note\":\"hi\",\"quantity\":2,\"detail\":{\"tenantId\":\"nested\"},\"tenantId\":[{\"note\":\"decoy\"}],\"dryRun\":{}}",
        "bodyMediaType": "application/json"
    }
])

apply CreateOrder @httpResponseTests([
    {
        "id": "BodyPayloadtenantA",
        "protocol": "aws.protocols#restJson1",
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
        "protocol": "aws.protocols#restJson1",
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
