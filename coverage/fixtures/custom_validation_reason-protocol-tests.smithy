$version: "2"
namespace coverage.fixtures.custom_validation_reason

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.customvalidation#CreateItem @smithy.test#httpRequestTests([
    {
        "id": "CoverageReasonValidationPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/item",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"name\":\"alice\",\"count\":17,\"color\":\"blue\",\"tags\":[\"alpha\",\"bravo\"],\"payload\":\"YmxvYg==\",\"attributes\":{\"first\":\"alpha\",\"second\":\"bravo\"},\"uniqueNumbers\":[3,7],\"choice\":{\"sized\":\"union\"}}",
        "bodyMediaType": "application/json",
        "params": {
            "name": "alice",
            "count": 17,
            "color": "blue",
            "tags": [
                "alpha",
                "bravo"
            ],
            "payload": "blob",
            "attributes": {
                "first": "alpha",
                "second": "bravo"
            },
            "uniqueNumbers": [
                3,
                7
            ],
            "choice": {
                "sized": "union"
            }
        },
        "appliesTo": "server"
    }
])

apply test.customvalidation#CreateItem @smithy.test#httpResponseTests([
    {
        "id": "CoverageReasonValidationPopulatedResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"id\":\"created-item\"}",
        "bodyMediaType": "application/json",
        "params": {
            "id": "created-item"
        },
        "appliesTo": "server"
    }
])

apply test.customvalidation#CreateItem @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageReasonRequiredMissing",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/item",
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
                    "contents": "{\"Message\":\"1 validation error detected. Value at '/name' failed to satisfy constraint: Member must not be null\",\"Reason\":\"FieldValidationFailed\",\"Fields\":[{\"Name\":\"/name\",\"Reason\":\"Other\",\"Message\":\"Value at '/name' failed to satisfy constraint: Member must not be null\"}]}"
                }
            }
        }
    }
])
