// coverage-service: test.endpoints#EndpointService
// coverage-plugins: client
$version: "2.0"

metadata suppressions = [
    {
        id: "RuleSetParameter.TestCase.Unused",
        namespace: "*",
        reason: "Coverage model deliberately declares params not used by every test case",
    },
]

namespace test.endpoints

use aws.protocols#restJson1
use smithy.rules#clientContextParams
use smithy.rules#contextParam
use smithy.rules#endpointRuleSet
use smithy.rules#endpointTests
use smithy.rules#operationContextParams
use smithy.rules#staticContextParams

@restJson1
@clientContextParams(
    Stage: { type: "string", documentation: "Deployment stage" }
    UseFips: { type: "boolean", documentation: "Use FIPS endpoints" }
)
@endpointRuleSet({
    version: "1.0"
    parameters: {
        Region: { required: true, type: "string", documentation: "Region", default: "us-default-1" }
        Stage: { required: false, type: "string", documentation: "Stage" }
        UseFips: { required: true, type: "boolean", default: false, documentation: "FIPS" }
        Endpoint: { required: false, type: "string", builtIn: "SDK::Endpoint", documentation: "Endpoint override" }
        Bucket: { required: false, type: "string", documentation: "Bucket" }
        ExtraTags: { required: false, type: "stringArray", documentation: "Tags" }
    }
    rules: [
        {
            documentation: "endpoint override with url parsing"
            conditions: [
                { fn: "isSet", argv: [{ ref: "Endpoint" }] }
                { fn: "parseURL", argv: [{ ref: "Endpoint" }], assign: "parsedUrl" }
            ]
            type: "endpoint"
            endpoint: {
                url: { fn: "getAttr", argv: [{ ref: "parsedUrl" }, "scheme"] }
                properties: {}
                headers: {}
            }
        }
        {
            documentation: "fips + valid host label + substring + uriEncode"
            conditions: [
                { fn: "booleanEquals", argv: [{ ref: "UseFips" }, true] }
                { fn: "isValidHostLabel", argv: [{ ref: "Region" }, false] }
                { fn: "isSet", argv: [{ ref: "Bucket" }] }
                { fn: "substring", argv: [{ ref: "Bucket" }, 0, 3, false], assign: "bucketPrefix" }
                { fn: "uriEncode", argv: [{ ref: "Bucket" }], assign: "encodedBucket" }
                { fn: "stringEquals", argv: [{ ref: "bucketPrefix" }, "abc"] }
            ]
            type: "endpoint"
            endpoint: {
                url: "https://{encodedBucket}.fips.{Region}.example.com"
                properties: {
                    authSchemes: [{ name: "sigv4", signingRegion: "{Region}" }]
                }
                headers: {
                    "x-amz-extra": ["{bucketPrefix}"]
                }
            }
        }
        {
            documentation: "tree rule with nested error and endpoint"
            conditions: [
                { fn: "isSet", argv: [{ ref: "Stage" }] }
            ]
            type: "tree"
            rules: [
                {
                    conditions: [
                        { fn: "stringEquals", argv: [{ ref: "Stage" }, "prod"] }
                    ]
                    type: "endpoint"
                    endpoint: { url: "https://prod.{Region}.example.com", properties: {}, headers: {} }
                }
                {
                    conditions: [
                        { fn: "not", argv: [{ fn: "stringEquals", argv: [{ ref: "Stage" }, "gamma"] }] }
                    ]
                    type: "error"
                    error: "unknown stage: `{Stage}`"
                }
                {
                    conditions: []
                    type: "endpoint"
                    endpoint: { url: "https://gamma.{Region}.example.com", properties: {}, headers: {} }
                }
            ]
        }
        {
            documentation: "fallback"
            conditions: []
            type: "endpoint"
            endpoint: { url: "https://{Region}.example.com", properties: {}, headers: {} }
        }
    ]
})
@endpointTests({
    version: "1.0"
    testCases: [
        {
            documentation: "default endpoint"
            params: { Region: "us-west-2" }
            expect: { endpoint: { url: "https://us-west-2.example.com" } }
        }
        {
            documentation: "prod stage"
            params: { Region: "us-west-2", Stage: "prod" }
            operationInputs: [
                { operationName: "GetThing", operationParams: { name: "hello" } }
            ]
            expect: { endpoint: { url: "https://prod.us-west-2.example.com" } }
        }
        {
            documentation: "fips endpoint with bucket"
            params: { Region: "us-west-2", UseFips: true, Bucket: "abcdef" }
            expect: {
                endpoint: {
                    url: "https://abcdef.fips.us-west-2.example.com"
                    properties: {
                        authSchemes: [{ name: "sigv4", signingRegion: "us-west-2" }]
                    }
                    headers: {
                        "x-amz-extra": ["abc"]
                    }
                }
            }
        }
        {
            documentation: "bad stage errors"
            params: { Region: "us-west-2", Stage: "beta" }
            expect: { error: "unknown stage: `beta`" }
        }
    ]
})
service EndpointService {
    version: "1.0"
    operations: [GetThing]
}

@staticContextParams(
    Region: { value: "us-static-1" }
    ExtraTags: { value: ["a", "b"] }
)
@operationContextParams(
    Stage: { path: "nested.stage" }
)
@http(method: "POST", uri: "/thing")
operation GetThing {
    input := {
        @required
        @contextParam(name: "Bucket")
        name: String
        nested: Nested
    }
    output := {
        id: String
    }
}

structure Nested {
    stage: String
}
