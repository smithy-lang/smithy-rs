$version: "1.0"

namespace aws.protocoltests.json

use aws.protocols#awsJson1_1
use smithy.test#httpRequestTests

apply JsonEnums @httpRequestTests([
    {
        id: "AwsJson11EnumsBasic"
        documentation: "Serializes simple scalar properties"
        protocol: awsJson1_1
        method: "POST"
        uri: "/"
        body: """
            {
            "fooEnum1": "Foo",
            "fooEnum2": "0",
            "fooEnum3": "1",
            "fooEnumList": [
            "Foo",
            "0"
            ],
            "fooEnumMap": {
            "hi": "Foo",
            "zero": "0"
            }
            }"""
        headers: { "Content-Type": "application/x-amz-json-1.1" }
        bodyMediaType: "application/json"
        params: {
            fooEnum1: "Foo"
            fooEnum2: "0"
            fooEnum3: "1"
            fooEnumList: ["Foo", "0"]
            fooEnumMap: { hi: "Foo", zero: "0" }
        }
    }
])
