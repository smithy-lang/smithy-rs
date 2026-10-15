$version: "2"
namespace coverage.fixtures.member_constraints

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.memberconstraints#PutMemberConstrained @smithy.test#httpRequestTests([
    {
        "id": "CoverageMemberConstraintsPopulatedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/member-constrained",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"name\":\"alice\",\"code\":\"abc\",\"names\":[\"one\",\"two\"],\"lookup\":{\"digits\":\"123\"},\"keyed\":{\"ab\":\"first\",\"cd\":\"second\"}}",
        "bodyMediaType": "application/json",
        "params": {
            "name": "alice",
            "code": "abc",
            "names": [
                "one",
                "two"
            ],
            "lookup": {
                "digits": "123"
            },
            "keyed": {
                "ab": "first",
                "cd": "second"
            }
        },
        "appliesTo": "server"
    }
])

apply test.memberconstraints#PutMemberConstrained @smithy.test#httpResponseTests([
    {
        "id": "CoverageMemberConstraintsPopulatedResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"id\":\"created-id\"}",
        "bodyMediaType": "application/json",
        "params": {
            "id": "created-id"
        },
        "appliesTo": "server"
    }
])

apply test.memberconstraints#PutMemberConstrained @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageMemberRequiredMissing",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/member-constrained",
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
    },
    {
        "id": "CoverageMemberNameEmpty",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/member-constrained",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"\"}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value with length 0 at '/name' failed to satisfy constraint: Member must have length between 1 and 16, inclusive\",\"fieldList\":[{\"message\":\"Value with length 0 at '/name' failed to satisfy constraint: Member must have length between 1 and 16, inclusive\",\"path\":\"/name\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageMemberPatternRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/member-constrained",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"alice\",\"code\":\"ABC\"}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value at '/code' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-z]+$\",\"fieldList\":[{\"message\":\"Value at '/code' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-z]+$\",\"path\":\"/code\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageMemberListMemberEmpty",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/member-constrained",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"alice\",\"names\":[\"\"]}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value with length 0 at '/names/0' failed to satisfy constraint: Member must have length between 1 and 8, inclusive\",\"fieldList\":[{\"message\":\"Value with length 0 at '/names/0' failed to satisfy constraint: Member must have length between 1 and 8, inclusive\",\"path\":\"/names/0\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageMemberMapValueRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/member-constrained",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"alice\",\"lookup\":{\"digits\":\"abc\"}}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value at '/lookup/digits' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[0-9]+$\",\"fieldList\":[{\"message\":\"Value at '/lookup/digits' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[0-9]+$\",\"path\":\"/lookup/digits\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageMemberMapKeyRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/member-constrained",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"name\":\"alice\",\"keyed\":{\"a\":\"value\"}}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value with length 1 at '/keyed' failed to satisfy constraint: Member must have length between 2 and 4, inclusive\",\"fieldList\":[{\"message\":\"Value with length 1 at '/keyed' failed to satisfy constraint: Member must have length between 2 and 4, inclusive\",\"path\":\"/keyed\"}]}"
                }
            }
        }
    }
])
