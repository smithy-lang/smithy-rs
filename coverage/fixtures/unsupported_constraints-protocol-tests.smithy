$version: "2"
namespace coverage.fixtures.unsupported_constraints

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.unsupported#PutStuff @smithy.test#httpRequestTests([
    {
        "id": "CoverageUnsupportedConstraintsPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/stuff",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"memberConstrainedString\":\"alice\",\"memberConstrainedInt\":3,\"memberConstrainedPattern\":\"alpha\",\"ratio\":0.25,\"bigRatio\":17.5}",
        "bodyMediaType": "application/json",
        "params": {
            "memberConstrainedString": "alice",
            "memberConstrainedInt": 3,
            "memberConstrainedPattern": "alpha",
            "ratio": 0.25,
            "bigRatio": 17.5
        },
        "appliesTo": "server"
    }
])

apply test.unsupported#PutStuff @smithy.test#httpResponseTests([
    {
        "id": "CoverageUnsupportedConstraintsPopulatedResponse",
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

apply test.unsupported#PutStuff @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageUnsupportedMemberLengthRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/stuff",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"memberConstrainedString\":\"\"}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value with length 0 at '/memberConstrainedString' failed to satisfy constraint: Member must have length between 1 and 10, inclusive\",\"fieldList\":[{\"message\":\"Value with length 0 at '/memberConstrainedString' failed to satisfy constraint: Member must have length between 1 and 10, inclusive\",\"path\":\"/memberConstrainedString\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageUnsupportedMemberRangeRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/stuff",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"memberConstrainedInt\":6}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value at '/memberConstrainedInt' failed to satisfy constraint: Member must be between 1 and 5, inclusive\",\"fieldList\":[{\"message\":\"Value at '/memberConstrainedInt' failed to satisfy constraint: Member must be between 1 and 5, inclusive\",\"path\":\"/memberConstrainedInt\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageUnsupportedMemberPatternRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/stuff",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"memberConstrainedPattern\":\"ABC\"}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value at '/memberConstrainedPattern' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-z]+$\",\"fieldList\":[{\"message\":\"Value at '/memberConstrainedPattern' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-z]+$\",\"path\":\"/memberConstrainedPattern\"}]}"
                }
            }
        }
    }
])
