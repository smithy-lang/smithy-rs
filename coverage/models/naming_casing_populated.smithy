// coverage-service: coverage.naming.casing#ACRONYMInside_Service
// coverage-plugins: client,server
$version: "2.0"
namespace coverage.naming.casing
use aws.protocols#awsJson1_1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

// Legal Smithy names copied from the commented naming obstacle course. They may
// collide after Rust name conversion; generation/compilation failures must remain
// visible. These declarations are isolated from the production model.
@awsJson1_1
service ACRONYMInside_Service {
    version: "1"
    operations: [ACRONYMInside_Op, ACRONYM_InsideOp, Match, Result, Option, RPCEcho]
}

structure CasingInputOutput {
    ACRONYMInside_Member: ACRONYMInside_Struct
    ACRONYM_Inside_Member: ACRONYM_InsideStruct
    ACRONYM_InsideMember: ACRONYMInsideStruct
    match: String
    type: String
    self: String
    crate: String
    async: Boolean
    build: String
    default: String
    send: String
    someUnion: CasingChoice
}
structure ACRONYMInside_Struct {
    ACRONYMInside_Member: ACRONYM_InsideStruct
    ACRONYM_Inside_Member: Integer
}
structure ACRONYM_InsideStruct { ACRONYMInside_Member: Integer }
structure ACRONYMInsideStruct { ACRONYMInside_Member: Integer }
structure KeywordResult { value: String }
structure KeywordOption { value: String }
union CasingChoice { Result: KeywordResult, Option: KeywordOption }

@httpRequestTests([
    { id: "PopulatedCasingACRONYMInsideUnderscoreOpRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.ACRONYMInside_Op" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Result\":{\"value\":\"result-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Result: { value: "result-value" } } } },
])
@httpResponseTests([
    { id: "PopulatedCasingACRONYMInsideUnderscoreOpResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Result\":{\"value\":\"result-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Result: { value: "result-value" } } } },
])
operation ACRONYMInside_Op { input: CasingInputOutput, output: CasingInputOutput }

@httpRequestTests([
    { id: "PopulatedCasingACRONYMUnderscoreInsideOpRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.ACRONYM_InsideOp" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Option\":{\"value\":\"option-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Option: { value: "option-value" } } } },
])
@httpResponseTests([
    { id: "PopulatedCasingACRONYMUnderscoreInsideOpResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Option\":{\"value\":\"option-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Option: { value: "option-value" } } } },
])
operation ACRONYM_InsideOp { input: CasingInputOutput, output: CasingInputOutput }

@httpRequestTests([
    { id: "PopulatedCasingMatchRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.Match" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Result\":{\"value\":\"result-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Result: { value: "result-value" } } } },
])
@httpResponseTests([
    { id: "PopulatedCasingMatchResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Result\":{\"value\":\"result-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Result: { value: "result-value" } } } },
])
operation Match { input: CasingInputOutput, output: CasingInputOutput }

@httpRequestTests([
    { id: "PopulatedCasingResultRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.Result" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Option\":{\"value\":\"option-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Option: { value: "option-value" } } } },
])
@httpResponseTests([
    { id: "PopulatedCasingResultResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Option\":{\"value\":\"option-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Option: { value: "option-value" } } } },
])
operation Result { input: CasingInputOutput, output: CasingInputOutput }

@httpRequestTests([
    { id: "PopulatedCasingOptionRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.Option" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Result\":{\"value\":\"result-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Result: { value: "result-value" } } } },
])
@httpResponseTests([
    { id: "PopulatedCasingOptionResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Result\":{\"value\":\"result-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Result: { value: "result-value" } } } },
])
operation Option { input: CasingInputOutput, output: CasingInputOutput }

@httpRequestTests([
    { id: "PopulatedCasingRPCEchoRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.RPCEcho" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Option\":{\"value\":\"option-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Option: { value: "option-value" } } } },
])
@httpResponseTests([
    { id: "PopulatedCasingRPCEchoResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":{\"ACRONYMInside_Member\":1},\"ACRONYM_Inside_Member\":2},\"ACRONYM_Inside_Member\":{\"ACRONYMInside_Member\":3},\"ACRONYM_InsideMember\":{\"ACRONYMInside_Member\":4},\"match\":\"match-value\",\"type\":\"type-value\",\"self\":\"self-value\",\"crate\":\"crate-value\",\"async\":true,\"build\":\"build-value\",\"default\":\"default-value\",\"send\":\"send-value\",\"someUnion\":{\"Option\":{\"value\":\"option-value\"}}}", bodyMediaType: "application/json", params: { ACRONYMInside_Member: { ACRONYMInside_Member: { ACRONYMInside_Member: 1 }, ACRONYM_Inside_Member: 2 }, ACRONYM_Inside_Member: { ACRONYMInside_Member: 3 }, ACRONYM_InsideMember: { ACRONYMInside_Member: 4 }, match: "match-value", type: "type-value", self: "self-value", crate: "crate-value", async: true, build: "build-value", default: "default-value", send: "send-value", someUnion: { Option: { value: "option-value" } } } },
])
operation RPCEcho { input: CasingInputOutput, output: CasingInputOutput }
