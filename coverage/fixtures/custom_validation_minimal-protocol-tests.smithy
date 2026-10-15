$version: "2"
namespace coverage.fixtures.custom_validation_minimal

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.customvalidationmin#PutThing @smithy.test#httpRequestTests([
    {
        "id": "CoverageMinimalValidationPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/thing",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"name\":\"alice\",\"count\":17}",
        "bodyMediaType": "application/json",
        "params": {
            "name": "alice",
            "count": 17
        },
        "appliesTo": "server"
    }
])

apply test.customvalidationmin#PutThing @smithy.test#httpResponseTests([
    {
        "id": "CoverageMinimalValidationPopulatedResponse",
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

apply test.customvalidationmin#PutThing @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageMinimalRequiredMissing",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/thing",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{}"
        },
        "response": {
            "code": 400,
            "headers": {
                "x-amzn-errortype": "MinimalValidationException",
                "content-type": "application/json"
            },
            "body": {
                "mediaType": "application/json",
                "assertion": {
                    "contents": "{\"customMessage\":\"validation error detected\"}"
                }
            }
        }
    },
    {
        "id": "CoverageMinimalLengthRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/thing",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"\"}"
        },
        "response": {
            "code": 400,
            "headers": {
                "x-amzn-errortype": "MinimalValidationException",
                "content-type": "application/json"
            },
            "body": {
                "mediaType": "application/json",
                "assertion": {
                    "contents": "{\"customMessage\":\"validation error detected\"}"
                }
            }
        }
    },
    {
        "id": "CoverageMinimalRangeRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/thing",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"alice\",\"count\":101}"
        },
        "response": {
            "code": 400,
            "headers": {
                "x-amzn-errortype": "MinimalValidationException",
                "content-type": "application/json"
            },
            "body": {
                "mediaType": "application/json",
                "assertion": {
                    "contents": "{\"customMessage\":\"validation error detected\"}"
                }
            }
        }
    }
])
