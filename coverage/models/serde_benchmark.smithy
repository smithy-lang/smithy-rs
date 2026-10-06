// coverage-service: test.serdebench#SerdeBenchService
// coverage-plugins: client
$version: "2.0"

namespace test.serdebench

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
service SerdeBenchService {
    version: "1.0"
    operations: [PutItem]
}

@http(method: "POST", uri: "/item")
@httpRequestTests([
    {
        id: "SerdeBenchRequest"
        documentation: "Benchmark request serialization"
        protocol: "aws.protocols#restJson1"
        method: "POST"
        uri: "/item"
        body: "{\"name\":\"foo\",\"count\":3}"
        bodyMediaType: "application/json"
        params: { name: "foo", count: 3 }
        tags: ["serde-benchmark"]
        appliesTo: "client"
    }
])
@httpResponseTests([
    {
        id: "SerdeBenchResponse"
        documentation: "Benchmark response deserialization"
        protocol: "aws.protocols#restJson1"
        code: 200
        body: "{\"id\":\"abc\",\"names\":[\"a\",\"b\"]}"
        bodyMediaType: "application/json"
        params: { id: "abc", names: ["a", "b"] }
        tags: ["serde-benchmark"]
        appliesTo: "client"
    }
])
operation PutItem {
    input := {
        name: String
        count: Integer
    }
    output := {
        id: String
        names: NameList
    }
}

list NameList {
    member: String
}
