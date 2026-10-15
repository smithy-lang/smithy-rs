$version: "2"
namespace coverage.fixtures.constraints

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply com.amazonaws.constraints#ConstrainedShapesOperation @smithy.test#httpRequestTests([
    {
        "id": "CoverageConstrainedNestedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/constrained-shapes-operation",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"conA\":{\"conB\":{\"nice\":\"nested\",\"int\":7},\"lengthString\":\"four\",\"rangeInteger\":17,\"minRangeInteger\":-3,\"maxRangeInteger\":19,\"fixedValueInteger\":69,\"rangeShort\":3,\"minRangeShort\":-3,\"maxRangeShort\":7,\"fixedValueShort\":10,\"rangeLong\":4,\"minRangeLong\":-4,\"maxRangeLong\":8,\"fixedValueLong\":10,\"rangeByte\":5,\"minRangeByte\":-5,\"maxRangeByte\":9,\"fixedValueByte\":10,\"lengthBlob\":\"YmxvYg==\",\"lengthList\":[\"alpha\",\"bravo\"],\"constrainedUnion\":{\"lengthString\":\"union\"},\"listOfLengthString\":[\"alpha\",\"bravo\"],\"mapOfLengthString\":{\"first\":\"alpha\",\"second\":\"bravo\"}}}",
        "bodyMediaType": "application/json",
        "params": {
            "conA": {
                "conB": {
                    "nice": "nested",
                    "int": 7
                },
                "lengthString": "four",
                "rangeInteger": 17,
                "minRangeInteger": -3,
                "maxRangeInteger": 19,
                "fixedValueInteger": 69,
                "rangeShort": 3,
                "minRangeShort": -3,
                "maxRangeShort": 7,
                "fixedValueShort": 10,
                "rangeLong": 4,
                "minRangeLong": -4,
                "maxRangeLong": 8,
                "fixedValueLong": 10,
                "rangeByte": 5,
                "minRangeByte": -5,
                "maxRangeByte": 9,
                "fixedValueByte": 10,
                "lengthBlob": "blob",
                "lengthList": [
                    "alpha",
                    "bravo"
                ],
                "constrainedUnion": {
                    "lengthString": "union"
                },
                "listOfLengthString": [
                    "alpha",
                    "bravo"
                ],
                "mapOfLengthString": {
                    "first": "alpha",
                    "second": "bravo"
                }
            }
        },
        "appliesTo": "server"
    }
])

apply com.amazonaws.constraints#ConstrainedShapesOperation @smithy.test#httpResponseTests([
    {
        "id": "CoverageConstrainedNestedResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"conA\":{\"conB\":{\"nice\":\"nested\",\"int\":7},\"lengthString\":\"four\",\"rangeInteger\":17,\"minRangeInteger\":-3,\"maxRangeInteger\":19,\"fixedValueInteger\":69,\"rangeShort\":3,\"minRangeShort\":-3,\"maxRangeShort\":7,\"fixedValueShort\":10,\"rangeLong\":4,\"minRangeLong\":-4,\"maxRangeLong\":8,\"fixedValueLong\":10,\"rangeByte\":5,\"minRangeByte\":-5,\"maxRangeByte\":9,\"fixedValueByte\":10,\"lengthBlob\":\"YmxvYg==\",\"lengthList\":[\"alpha\",\"bravo\"],\"constrainedUnion\":{\"lengthString\":\"union\"},\"listOfLengthString\":[\"alpha\",\"bravo\"],\"mapOfLengthString\":{\"first\":\"alpha\",\"second\":\"bravo\"}}}",
        "bodyMediaType": "application/json",
        "params": {
            "conA": {
                "conB": {
                    "nice": "nested",
                    "int": 7
                },
                "lengthString": "four",
                "rangeInteger": 17,
                "minRangeInteger": -3,
                "maxRangeInteger": 19,
                "fixedValueInteger": 69,
                "rangeShort": 3,
                "minRangeShort": -3,
                "maxRangeShort": 7,
                "fixedValueShort": 10,
                "rangeLong": 4,
                "minRangeLong": -4,
                "maxRangeLong": 8,
                "fixedValueLong": 10,
                "rangeByte": 5,
                "minRangeByte": -5,
                "maxRangeByte": 9,
                "fixedValueByte": 10,
                "lengthBlob": "blob",
                "lengthList": [
                    "alpha",
                    "bravo"
                ],
                "constrainedUnion": {
                    "lengthString": "union"
                },
                "listOfLengthString": [
                    "alpha",
                    "bravo"
                ],
                "mapOfLengthString": {
                    "first": "alpha",
                    "second": "bravo"
                }
            }
        },
        "appliesTo": "server"
    }
])

apply com.amazonaws.constraints#ConstrainedShapesOperation @smithy.test#httpMalformedRequestTests([
    {
        "id": "CoverageConstrainedRequiredMissing",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/constrained-shapes-operation",
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
                    "contents": "{\"message\":\"1 validation error detected. Value at '/conA' failed to satisfy constraint: Member must not be null\",\"fieldList\":[{\"message\":\"Value at '/conA' failed to satisfy constraint: Member must not be null\",\"path\":\"/conA\"}]}"
                }
            }
        }
    },
    {
        "id": "CoverageConstrainedLengthRejected",
        "documentation": "Reject this isolated constraint violation before invoking the operation handler. The explicit diagnostic body uses the repository's validation-exception contract.",
        "protocol": "aws.protocols#restJson1",
        "request": {
            "method": "POST",
            "uri": "/constrained-shapes-operation",
            "headers": {
                "content-type": "application/json"
            },
            "body": "{\"conA\":{\"conB\":{\"nice\":\"nested\",\"int\":7},\"lengthString\":\"x\",\"rangeInteger\":17,\"minRangeInteger\":-3,\"maxRangeInteger\":19,\"fixedValueInteger\":69,\"rangeShort\":3,\"minRangeShort\":-3,\"maxRangeShort\":7,\"fixedValueShort\":10,\"rangeLong\":4,\"minRangeLong\":-4,\"maxRangeLong\":8,\"fixedValueLong\":10,\"rangeByte\":5,\"minRangeByte\":-5,\"maxRangeByte\":9,\"fixedValueByte\":10,\"lengthBlob\":\"blob\",\"lengthList\":[\"alpha\",\"bravo\"],\"constrainedUnion\":{\"lengthString\":\"union\"},\"listOfLengthString\":[\"alpha\",\"bravo\"],\"mapOfLengthString\":{\"first\":\"alpha\",\"second\":\"bravo\"}}}"
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
                    "contents": "{\"message\":\"1 validation error detected. Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive\",\"fieldList\":[{\"message\":\"Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive\",\"path\":\"/conA/lengthString\"}]}"
                }
            }
        }
    }
])

apply com.amazonaws.constraints#ConstrainedHttpPayloadBoundShapeOperation @smithy.test#httpRequestTests([
    {
        "id": "CoverageConstrainedPayloadRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/constrained-http-payload-bound-shape-operation",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"conB\":{\"nice\":\"nested\",\"int\":7},\"lengthString\":\"four\",\"rangeInteger\":17,\"minRangeInteger\":-3,\"maxRangeInteger\":19,\"fixedValueInteger\":69,\"rangeShort\":3,\"minRangeShort\":-3,\"maxRangeShort\":7,\"fixedValueShort\":10,\"rangeLong\":4,\"minRangeLong\":-4,\"maxRangeLong\":8,\"fixedValueLong\":10,\"rangeByte\":5,\"minRangeByte\":-5,\"maxRangeByte\":9,\"fixedValueByte\":10,\"lengthBlob\":\"YmxvYg==\",\"lengthList\":[\"alpha\",\"bravo\"],\"constrainedUnion\":{\"lengthString\":\"union\"},\"listOfLengthString\":[\"alpha\",\"bravo\"],\"mapOfLengthString\":{\"first\":\"alpha\",\"second\":\"bravo\"}}",
        "bodyMediaType": "application/json",
        "params": {
            "httpPayloadBoundConstrainedShape": {
                "conB": {
                    "nice": "nested",
                    "int": 7
                },
                "lengthString": "four",
                "rangeInteger": 17,
                "minRangeInteger": -3,
                "maxRangeInteger": 19,
                "fixedValueInteger": 69,
                "rangeShort": 3,
                "minRangeShort": -3,
                "maxRangeShort": 7,
                "fixedValueShort": 10,
                "rangeLong": 4,
                "minRangeLong": -4,
                "maxRangeLong": 8,
                "fixedValueLong": 10,
                "rangeByte": 5,
                "minRangeByte": -5,
                "maxRangeByte": 9,
                "fixedValueByte": 10,
                "lengthBlob": "blob",
                "lengthList": [
                    "alpha",
                    "bravo"
                ],
                "constrainedUnion": {
                    "lengthString": "union"
                },
                "listOfLengthString": [
                    "alpha",
                    "bravo"
                ],
                "mapOfLengthString": {
                    "first": "alpha",
                    "second": "bravo"
                }
            }
        },
        "appliesTo": "server"
    }
])

apply com.amazonaws.constraints#ConstrainedHttpPayloadBoundShapeOperation @smithy.test#httpResponseTests([
    {
        "id": "CoverageConstrainedPayloadResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"conB\":{\"nice\":\"nested\",\"int\":7},\"lengthString\":\"four\",\"rangeInteger\":17,\"minRangeInteger\":-3,\"maxRangeInteger\":19,\"fixedValueInteger\":69,\"rangeShort\":3,\"minRangeShort\":-3,\"maxRangeShort\":7,\"fixedValueShort\":10,\"rangeLong\":4,\"minRangeLong\":-4,\"maxRangeLong\":8,\"fixedValueLong\":10,\"rangeByte\":5,\"minRangeByte\":-5,\"maxRangeByte\":9,\"fixedValueByte\":10,\"lengthBlob\":\"YmxvYg==\",\"lengthList\":[\"alpha\",\"bravo\"],\"constrainedUnion\":{\"lengthString\":\"union\"},\"listOfLengthString\":[\"alpha\",\"bravo\"],\"mapOfLengthString\":{\"first\":\"alpha\",\"second\":\"bravo\"}}",
        "bodyMediaType": "application/json",
        "params": {
            "httpPayloadBoundConstrainedShape": {
                "conB": {
                    "nice": "nested",
                    "int": 7
                },
                "lengthString": "four",
                "rangeInteger": 17,
                "minRangeInteger": -3,
                "maxRangeInteger": 19,
                "fixedValueInteger": 69,
                "rangeShort": 3,
                "minRangeShort": -3,
                "maxRangeShort": 7,
                "fixedValueShort": 10,
                "rangeLong": 4,
                "minRangeLong": -4,
                "maxRangeLong": 8,
                "fixedValueLong": 10,
                "rangeByte": 5,
                "minRangeByte": -5,
                "maxRangeByte": 9,
                "fixedValueByte": 10,
                "lengthBlob": "blob",
                "lengthList": [
                    "alpha",
                    "bravo"
                ],
                "constrainedUnion": {
                    "lengthString": "union"
                },
                "listOfLengthString": [
                    "alpha",
                    "bravo"
                ],
                "mapOfLengthString": {
                    "first": "alpha",
                    "second": "bravo"
                }
            }
        },
        "appliesTo": "server"
    }
])

apply com.amazonaws.constraints#ConstrainedRecursiveShapesOperation @smithy.test#httpRequestTests([
    {
        "id": "CoverageConstrainedRecursiveRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/constrained-recursive-shapes-operation",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"recursiveList\":[{\"recursiveMember\":{\"recursiveMember\":{\"recursiveMember\":{}}}}],\"nested\":{\"recursiveMember\":{}}}",
        "bodyMediaType": "application/json",
        "params": {
            "recursiveList": [
                {
                    "recursiveMember": {
                        "recursiveMember": {
                            "recursiveMember": {}
                        }
                    }
                }
            ],
            "nested": {
                "recursiveMember": {}
            }
        },
        "appliesTo": "server"
    }
])

apply com.amazonaws.constraints#ConstrainedRecursiveShapesOperation @smithy.test#httpResponseTests([
    {
        "id": "CoverageConstrainedRecursiveResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"recursiveList\":[{\"recursiveMember\":{\"recursiveMember\":{\"recursiveMember\":{}}}}],\"nested\":{\"recursiveMember\":{}}}",
        "bodyMediaType": "application/json",
        "params": {
            "recursiveList": [
                {
                    "recursiveMember": {
                        "recursiveMember": {
                            "recursiveMember": {}
                        }
                    }
                }
            ],
            "nested": {
                "recursiveMember": {}
            }
        },
        "appliesTo": "server"
    }
])

apply com.amazonaws.constraints#ConstrainedShapesOnlyInOutputOperation @smithy.test#httpResponseTests([
    {
        "id": "CoverageOutputOnlyConstraintsPopulatedResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"list\":[{\"structure\":{\"lengthString\":\"list-00\"}},{\"structure\":{\"lengthString\":\"list-01\"}},{\"structure\":{\"lengthString\":\"list-02\"}},{\"structure\":{\"lengthString\":\"list-03\"}},{\"structure\":{\"lengthString\":\"list-04\"}},{\"structure\":{\"lengthString\":\"list-05\"}},{\"structure\":{\"lengthString\":\"list-06\"}},{\"structure\":{\"lengthString\":\"list-07\"}},{\"structure\":{\"lengthString\":\"list-08\"}},{\"structure\":{\"lengthString\":\"list-09\"}},{\"structure\":{\"lengthString\":\"list-10\"}},{\"structure\":{\"lengthString\":\"list-11\"}},{\"structure\":{\"lengthString\":\"list-12\"}},{\"structure\":{\"lengthString\":\"list-13\"}},{\"structure\":{\"lengthString\":\"list-14\"}},{\"structure\":{\"lengthString\":\"list-15\"}},{\"structure\":{\"lengthString\":\"list-16\"}},{\"structure\":{\"lengthString\":\"list-17\"}},{\"structure\":{\"lengthString\":\"list-18\"}},{\"structure\":{\"lengthString\":\"list-19\"}},{\"structure\":{\"lengthString\":\"list-20\"}},{\"structure\":{\"lengthString\":\"list-21\"}},{\"structure\":{\"lengthString\":\"list-22\"}},{\"structure\":{\"lengthString\":\"list-23\"}},{\"structure\":{\"lengthString\":\"list-24\"}},{\"structure\":{\"lengthString\":\"list-25\"}},{\"structure\":{\"lengthString\":\"list-26\"}},{\"structure\":{\"lengthString\":\"list-27\"}},{\"structure\":{\"lengthString\":\"list-28\"}},{\"structure\":{\"lengthString\":\"list-29\"}},{\"structure\":{\"lengthString\":\"list-30\"}},{\"structure\":{\"lengthString\":\"list-31\"}},{\"structure\":{\"lengthString\":\"list-32\"}},{\"structure\":{\"lengthString\":\"list-33\"}},{\"structure\":{\"lengthString\":\"list-34\"}},{\"structure\":{\"lengthString\":\"list-35\"}},{\"structure\":{\"lengthString\":\"list-36\"}},{\"structure\":{\"lengthString\":\"list-37\"}},{\"structure\":{\"lengthString\":\"list-38\"}},{\"structure\":{\"lengthString\":\"list-39\"}},{\"structure\":{\"lengthString\":\"list-40\"}},{\"structure\":{\"lengthString\":\"list-41\"}},{\"structure\":{\"lengthString\":\"list-42\"}},{\"structure\":{\"lengthString\":\"list-43\"}},{\"structure\":{\"lengthString\":\"list-44\"}},{\"structure\":{\"lengthString\":\"list-45\"}},{\"structure\":{\"lengthString\":\"list-46\"}},{\"structure\":{\"lengthString\":\"list-47\"}},{\"structure\":{\"lengthString\":\"list-48\"}},{\"structure\":{\"lengthString\":\"list-49\"}},{\"structure\":{\"lengthString\":\"list-50\"}},{\"structure\":{\"lengthString\":\"list-51\"}},{\"structure\":{\"lengthString\":\"list-52\"}},{\"structure\":{\"lengthString\":\"list-53\"}},{\"structure\":{\"lengthString\":\"list-54\"}},{\"structure\":{\"lengthString\":\"list-55\"}},{\"structure\":{\"lengthString\":\"list-56\"}},{\"structure\":{\"lengthString\":\"list-57\"}},{\"structure\":{\"lengthString\":\"list-58\"}},{\"structure\":{\"lengthString\":\"list-59\"}},{\"structure\":{\"lengthString\":\"list-60\"}},{\"structure\":{\"lengthString\":\"list-61\"}},{\"structure\":{\"lengthString\":\"list-62\"}},{\"structure\":{\"lengthString\":\"list-63\"}},{\"structure\":{\"lengthString\":\"list-64\"}},{\"structure\":{\"lengthString\":\"list-65\"}},{\"structure\":{\"lengthString\":\"list-66\"}},{\"structure\":{\"lengthString\":\"list-67\"}},{\"structure\":{\"lengthString\":\"list-68\"}}],\"map\":{\"key-00\":{\"lengthString\":\"value-00\"},\"key-01\":{\"lengthString\":\"value-01\"},\"key-02\":{\"lengthString\":\"value-02\"},\"key-03\":{\"lengthString\":\"value-03\"},\"key-04\":{\"lengthString\":\"value-04\"},\"key-05\":{\"lengthString\":\"value-05\"},\"key-06\":{\"lengthString\":\"value-06\"},\"key-07\":{\"lengthString\":\"value-07\"},\"key-08\":{\"lengthString\":\"value-08\"},\"key-09\":{\"lengthString\":\"value-09\"},\"key-10\":{\"lengthString\":\"value-10\"},\"key-11\":{\"lengthString\":\"value-11\"},\"key-12\":{\"lengthString\":\"value-12\"},\"key-13\":{\"lengthString\":\"value-13\"},\"key-14\":{\"lengthString\":\"value-14\"},\"key-15\":{\"lengthString\":\"value-15\"},\"key-16\":{\"lengthString\":\"value-16\"},\"key-17\":{\"lengthString\":\"value-17\"},\"key-18\":{\"lengthString\":\"value-18\"},\"key-19\":{\"lengthString\":\"value-19\"},\"key-20\":{\"lengthString\":\"value-20\"},\"key-21\":{\"lengthString\":\"value-21\"},\"key-22\":{\"lengthString\":\"value-22\"},\"key-23\":{\"lengthString\":\"value-23\"},\"key-24\":{\"lengthString\":\"value-24\"},\"key-25\":{\"lengthString\":\"value-25\"},\"key-26\":{\"lengthString\":\"value-26\"},\"key-27\":{\"lengthString\":\"value-27\"},\"key-28\":{\"lengthString\":\"value-28\"},\"key-29\":{\"lengthString\":\"value-29\"},\"key-30\":{\"lengthString\":\"value-30\"},\"key-31\":{\"lengthString\":\"value-31\"},\"key-32\":{\"lengthString\":\"value-32\"},\"key-33\":{\"lengthString\":\"value-33\"},\"key-34\":{\"lengthString\":\"value-34\"},\"key-35\":{\"lengthString\":\"value-35\"},\"key-36\":{\"lengthString\":\"value-36\"},\"key-37\":{\"lengthString\":\"value-37\"},\"key-38\":{\"lengthString\":\"value-38\"},\"key-39\":{\"lengthString\":\"value-39\"},\"key-40\":{\"lengthString\":\"value-40\"},\"key-41\":{\"lengthString\":\"value-41\"},\"key-42\":{\"lengthString\":\"value-42\"},\"key-43\":{\"lengthString\":\"value-43\"},\"key-44\":{\"lengthString\":\"value-44\"},\"key-45\":{\"lengthString\":\"value-45\"},\"key-46\":{\"lengthString\":\"value-46\"},\"key-47\":{\"lengthString\":\"value-47\"},\"key-48\":{\"lengthString\":\"value-48\"},\"key-49\":{\"lengthString\":\"value-49\"},\"key-50\":{\"lengthString\":\"value-50\"},\"key-51\":{\"lengthString\":\"value-51\"},\"key-52\":{\"lengthString\":\"value-52\"},\"key-53\":{\"lengthString\":\"value-53\"},\"key-54\":{\"lengthString\":\"value-54\"},\"key-55\":{\"lengthString\":\"value-55\"},\"key-56\":{\"lengthString\":\"value-56\"},\"key-57\":{\"lengthString\":\"value-57\"},\"key-58\":{\"lengthString\":\"value-58\"},\"key-59\":{\"lengthString\":\"value-59\"},\"key-60\":{\"lengthString\":\"value-60\"},\"key-61\":{\"lengthString\":\"value-61\"},\"key-62\":{\"lengthString\":\"value-62\"},\"key-63\":{\"lengthString\":\"value-63\"},\"key-64\":{\"lengthString\":\"value-64\"},\"key-65\":{\"lengthString\":\"value-65\"},\"key-66\":{\"lengthString\":\"value-66\"},\"key-67\":{\"lengthString\":\"value-67\"},\"key-68\":{\"lengthString\":\"value-68\"}},\"union\":{\"structure\":{\"lengthString\":\"union-value\"}}}",
        "bodyMediaType": "application/json",
        "params": {
            "list": [
                {
                    "structure": {
                        "lengthString": "list-00"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-01"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-02"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-03"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-04"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-05"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-06"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-07"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-08"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-09"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-10"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-11"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-12"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-13"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-14"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-15"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-16"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-17"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-18"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-19"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-20"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-21"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-22"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-23"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-24"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-25"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-26"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-27"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-28"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-29"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-30"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-31"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-32"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-33"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-34"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-35"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-36"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-37"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-38"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-39"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-40"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-41"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-42"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-43"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-44"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-45"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-46"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-47"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-48"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-49"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-50"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-51"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-52"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-53"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-54"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-55"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-56"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-57"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-58"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-59"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-60"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-61"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-62"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-63"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-64"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-65"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-66"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-67"
                    }
                },
                {
                    "structure": {
                        "lengthString": "list-68"
                    }
                }
            ],
            "map": {
                "key-00": {
                    "lengthString": "value-00"
                },
                "key-01": {
                    "lengthString": "value-01"
                },
                "key-02": {
                    "lengthString": "value-02"
                },
                "key-03": {
                    "lengthString": "value-03"
                },
                "key-04": {
                    "lengthString": "value-04"
                },
                "key-05": {
                    "lengthString": "value-05"
                },
                "key-06": {
                    "lengthString": "value-06"
                },
                "key-07": {
                    "lengthString": "value-07"
                },
                "key-08": {
                    "lengthString": "value-08"
                },
                "key-09": {
                    "lengthString": "value-09"
                },
                "key-10": {
                    "lengthString": "value-10"
                },
                "key-11": {
                    "lengthString": "value-11"
                },
                "key-12": {
                    "lengthString": "value-12"
                },
                "key-13": {
                    "lengthString": "value-13"
                },
                "key-14": {
                    "lengthString": "value-14"
                },
                "key-15": {
                    "lengthString": "value-15"
                },
                "key-16": {
                    "lengthString": "value-16"
                },
                "key-17": {
                    "lengthString": "value-17"
                },
                "key-18": {
                    "lengthString": "value-18"
                },
                "key-19": {
                    "lengthString": "value-19"
                },
                "key-20": {
                    "lengthString": "value-20"
                },
                "key-21": {
                    "lengthString": "value-21"
                },
                "key-22": {
                    "lengthString": "value-22"
                },
                "key-23": {
                    "lengthString": "value-23"
                },
                "key-24": {
                    "lengthString": "value-24"
                },
                "key-25": {
                    "lengthString": "value-25"
                },
                "key-26": {
                    "lengthString": "value-26"
                },
                "key-27": {
                    "lengthString": "value-27"
                },
                "key-28": {
                    "lengthString": "value-28"
                },
                "key-29": {
                    "lengthString": "value-29"
                },
                "key-30": {
                    "lengthString": "value-30"
                },
                "key-31": {
                    "lengthString": "value-31"
                },
                "key-32": {
                    "lengthString": "value-32"
                },
                "key-33": {
                    "lengthString": "value-33"
                },
                "key-34": {
                    "lengthString": "value-34"
                },
                "key-35": {
                    "lengthString": "value-35"
                },
                "key-36": {
                    "lengthString": "value-36"
                },
                "key-37": {
                    "lengthString": "value-37"
                },
                "key-38": {
                    "lengthString": "value-38"
                },
                "key-39": {
                    "lengthString": "value-39"
                },
                "key-40": {
                    "lengthString": "value-40"
                },
                "key-41": {
                    "lengthString": "value-41"
                },
                "key-42": {
                    "lengthString": "value-42"
                },
                "key-43": {
                    "lengthString": "value-43"
                },
                "key-44": {
                    "lengthString": "value-44"
                },
                "key-45": {
                    "lengthString": "value-45"
                },
                "key-46": {
                    "lengthString": "value-46"
                },
                "key-47": {
                    "lengthString": "value-47"
                },
                "key-48": {
                    "lengthString": "value-48"
                },
                "key-49": {
                    "lengthString": "value-49"
                },
                "key-50": {
                    "lengthString": "value-50"
                },
                "key-51": {
                    "lengthString": "value-51"
                },
                "key-52": {
                    "lengthString": "value-52"
                },
                "key-53": {
                    "lengthString": "value-53"
                },
                "key-54": {
                    "lengthString": "value-54"
                },
                "key-55": {
                    "lengthString": "value-55"
                },
                "key-56": {
                    "lengthString": "value-56"
                },
                "key-57": {
                    "lengthString": "value-57"
                },
                "key-58": {
                    "lengthString": "value-58"
                },
                "key-59": {
                    "lengthString": "value-59"
                },
                "key-60": {
                    "lengthString": "value-60"
                },
                "key-61": {
                    "lengthString": "value-61"
                },
                "key-62": {
                    "lengthString": "value-62"
                },
                "key-63": {
                    "lengthString": "value-63"
                },
                "key-64": {
                    "lengthString": "value-64"
                },
                "key-65": {
                    "lengthString": "value-65"
                },
                "key-66": {
                    "lengthString": "value-66"
                },
                "key-67": {
                    "lengthString": "value-67"
                },
                "key-68": {
                    "lengthString": "value-68"
                }
            },
            "union": {
                "structure": {
                    "lengthString": "union-value"
                }
            }
        },
        "appliesTo": "server"
    }
])
