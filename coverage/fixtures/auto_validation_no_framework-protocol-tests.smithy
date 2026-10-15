$version: "2"
namespace coverage.fixtures.auto_validation_no_framework

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.autovalidation#MakeIt @smithy.test#httpRequestTests([
    {
        "id": "CoverageAutoValidationPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/make",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"name\":\"created\",\"level\":7}",
        "bodyMediaType": "application/json",
        "params": {
            "name": "created",
            "level": 7
        },
        "appliesTo": "server"
    }
])

apply test.autovalidation#MakeIt @smithy.test#httpResponseTests([
    {
        "id": "CoverageAutoValidationPopulatedResponse",
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

apply test.autovalidation#MakeIt @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageAutoValidationRequiredMissing",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/make",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{}"
        },
        "response": {
            "code": 400,
            "headers": {
                "x-amzn-errortype": "ValidationException",
                "content-type": "application/json"
            },
            "body": {
                "mediaType": "application/json",
                "assertion": {
                    "contents": "{\"message\":\"1 validation error detected. Value at '/name' failed to satisfy constraint: Member must not be null\",\"fieldList\":[{\"message\":\"Value at '/name' failed to satisfy constraint: Member must not be null\",\"path\":\"/name\"}]}"
                }
            }
        }
    }
])
