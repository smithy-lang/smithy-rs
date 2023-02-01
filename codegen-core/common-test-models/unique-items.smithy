// TODO(https://github.com/awslabs/smithy/issues/1541): This file is
// temporary; these tests should really be in awslabs/smithy, but they were removed.
// They have been written starting off from the commit that removed them:
// https://github.com/awslabs/smithy/commit/1b5737a873033a101066b3d92b9e11d4ed3587c7

$version: "1.0"

namespace com.amazonaws.constraints

use aws.protocols#restJson1
use smithy.framework#ValidationException
use smithy.test#httpMalformedRequestTests

@restJson1
service UniqueItemsService {
    operations: [
        MalformedUniqueItems
    ]
}

@http(uri: "/MalformedUniqueItems", method: "POST")
operation MalformedUniqueItems {
    input: MalformedUniqueItemsInput,
    errors: [ValidationException]
}

apply MalformedUniqueItems @httpMalformedRequestTests([
    {
        id: "RestJsonMalformedUniqueItemsDuplicateItems",
        documentation: """
        When the list has duplicated items, the response should be a 400
        ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedUniqueItems",
            body: """
            { "set" : ["a", "a", "b", "c"] }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value with repeated values at indices [0, 1] at '/set' failed to satisfy constraint: Member must have unique values",
                      "fieldList" : [{"message": "Value with repeated values at indices [0, 1] at '/set' failed to satisfy constraint: Member must have unique values", "path": "/set"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedUniqueItemsDuplicateBlobs",
        documentation: """
        When the list has duplicated blobs, the response should be a 400
        ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedUniqueItems",
            body: """
            { "complexSet" : [{"foo": true, "blob": "YmxvYg=="}, {"foo": true, "blob": "b3RoZXJibG9i"}, {"foo": true, "blob": "YmxvYg=="}] }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value with repeated values at indices [0, 2] at '/complexSet' failed to satisfy constraint: Member must have unique values",
                      "fieldList" : [{"message": "Value with repeated values at indices [0, 2] at '/complexSet' failed to satisfy constraint: Member must have unique values", "path": "/complexSet"}]}"""
                }
            }
        }
    },
    // Note that the previously existing `RestJsonMalformedUniqueItemsNullItem` test is already covered by `RestJsonMalformedUniqueItemsNullItem`.
    // See https://github.com/awslabs/smithy/issues/1577#issuecomment-1397069720.
])

structure MalformedUniqueItemsInput {
    set: SimpleSet,
    complexSet: ComplexSet
}

@uniqueItems
list SimpleSet {
    member: String
}

@uniqueItems
list ComplexSet {
    member: ComplexSetStruct
}

structure ComplexSetStruct {
    foo: Boolean,
    blob: Blob
}
