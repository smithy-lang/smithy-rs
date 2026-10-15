$version: "2"
namespace coverage.fixtures.custom_validation_rich

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.customvalidationrich#PutWidget @smithy.test#httpRequestTests([
    {
        "id": "CoverageRichValidationPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/widget/widget-7",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"title\":\"A title\",\"offsetByte\":-3,\"offsetShort\":17,\"offsetLong\":23,\"color\":\"green\",\"rating\":5,\"tags\":[\"One\",\"Two\"],\"attributes\":{\"First\":\"Value\",\"Second\":\"Other\"},\"payload\":\"YmxvYg==\",\"uniqueNumbers\":[3,7],\"choice\":{\"pattern\":\"Union\"}}",
        "bodyMediaType": "application/json",
        "params": {
            "id": "widget-7",
            "title": "A title",
            "offsetByte": -3,
            "offsetShort": 17,
            "offsetLong": 23,
            "color": "green",
            "rating": 5,
            "tags": [
                "One",
                "Two"
            ],
            "attributes": {
                "First": "Value",
                "Second": "Other"
            },
            "payload": "blob",
            "uniqueNumbers": [
                3,
                7
            ],
            "choice": {
                "pattern": "Union"
            }
        },
        "appliesTo": "server"
    }
])

apply test.customvalidationrich#PutWidget @smithy.test#httpResponseTests([
    {
        "id": "CoverageRichValidationPopulatedResponse",
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

apply test.customvalidationrich#PutWidget @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageRichTitleLengthRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/widget/widget-7",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"title\":\"A\"}"
        },
        "response": {
            "code": 400,
            "headers": {
                "x-amzn-errortype": "MyValidationException",
                "content-type": "application/json"
            },
            "body": {
                "mediaType": "application/json",
                "assertion": {
                    "contents": "{\"customMessage\":\"validation error detected. Value with length 1 at '/title' failed to satisfy constraint: Member must have length between 2 and 64, inclusive\",\"customFieldList\":[{\"customFieldName\":\"/title\",\"customFieldMessage\":\"Value with length 1 at '/title' failed to satisfy constraint: Member must have length between 2 and 64, inclusive\"}],\"source\":\"client\",\"retryable\":true,\"attempts\":3,\"severity\":\"low\"}"
                }
            }
        }
    }
])
