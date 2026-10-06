// coverage-service: test.serverprototests#ServerProtoTestService
// coverage-plugins: server
$version: "2.0"

namespace test.serverprototests

use aws.protocols#restJson1
use smithy.framework#ValidationException
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
service ServerProtoTestService {
    version: "1.0"
    operations: [CreateGadget]
}

@http(method: "POST", uri: "/gadget")
@httpRequestTests([
    {
        id: "ServerCreateGadgetRequest"
        documentation: "Server instantiates constrained input from test params"
        protocol: "aws.protocols#restJson1"
        method: "POST"
        uri: "/gadget"
        body: "{\"name\":\"abc\",\"count\":2,\"kind\":\"big\",\"labels\":[\"one\"],\"meta\":{\"k\":\"v\"}}"
        bodyMediaType: "application/json"
        params: {
            name: "abc"
            count: 2
            kind: "big"
            labels: ["one"]
            meta: { k: "v" }
        }
        appliesTo: "server"
    }
])
@httpResponseTests([
    {
        id: "ServerCreateGadgetResponse"
        documentation: "Server serializes output with required and constrained members"
        protocol: "aws.protocols#restJson1"
        code: 201
        body: "{\"id\":\"gadget-1\",\"status\":\"ready\",\"tags\":[\"a\",\"b\"]}"
        bodyMediaType: "application/json"
        params: {
            id: "gadget-1"
            status: "ready"
            tags: ["a", "b"]
        }
        appliesTo: "server"
    }
])
operation CreateGadget {
    input := {
        @required
        @length(min: 1, max: 10)
        name: GadgetName

        @range(min: 1, max: 100)
        count: Integer

        kind: Kind

        labels: LabelList

        meta: MetaMap
    }
    output := {
        @required
        id: String

        @required
        status: GadgetName

        tags: LabelList
    }
    errors: [ValidationException]
}

@length(min: 1, max: 20)
string GadgetName

enum Kind {
    BIG = "big"
    SMALL = "small"
}

@length(min: 1, max: 10)
list LabelList {
    member: String
}

map MetaMap {
    key: String
    value: String
}
