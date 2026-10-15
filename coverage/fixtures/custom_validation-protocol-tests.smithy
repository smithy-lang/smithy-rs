$version: "2"
namespace coverage.fixtures.custom_validation

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply com.aws.example#TestOperation @smithy.test#httpRequestTests([
    {
        "id": "CoverageCustomValidationPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/test",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"name\":\"alice\",\"age\":17}",
        "bodyMediaType": "application/json",
        "params": {
            "name": "alice",
            "age": 17
        },
        "appliesTo": "server"
    }
])

apply com.aws.example#TestOperation @smithy.test#httpResponseTests([
    {
        "id": "CoverageCustomValidationEmptyResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "",
        "bodyMediaType": "application/json",
        "params": {},
        "appliesTo": "server"
    }
])

apply com.aws.example#TestOperation @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageCustomValidationRequiredMissing",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/test",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{}"
        },
        "response": {
            "code": 400,
            "headers": {
                "x-amzn-errortype": "MyCustomValidationException",
                "content-type": "application/json"
            },
            "body": {
                "mediaType": "application/json",
                "assertion": {
                    "contents": "{\"customMessage\":\"validation error detected. Value at '/name' failed to satisfy constraint: Member must not be null\",\"reason\":\"testReason1\",\"customFieldList\":[{\"customFieldName\":\"/name\",\"customFieldMessage\":\"Value at '/name' failed to satisfy constraint: Member must not be null\"}]}"
                }
            }
        }
    }
])
