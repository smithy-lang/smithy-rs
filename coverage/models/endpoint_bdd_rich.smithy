// coverage-service: test.bddrich#BddRichService
// coverage-plugins: client
$version: "2.0"

namespace test.bddrich

use aws.protocols#restJson1
use smithy.rules#clientContextParams
use smithy.rules#endpointBdd

@clientContextParams(
    Region: { type: "string", documentation: "Region" }
    Stage: { type: "string", documentation: "Deployment stage" }
    UseFips: { type: "boolean", documentation: "Use FIPS" }
    Bucket: { type: "string", documentation: "Bucket" }
)
@endpointBdd({
    version: "1.1"
    parameters: {
        Region: { required: true, type: "string", default: "us-east-1", documentation: "Region" }
        Endpoint: { required: false, type: "string", builtIn: "SDK::Endpoint", documentation: "Override" }
        UseFips: { required: true, type: "boolean", default: false, documentation: "FIPS" }
        Bucket: { required: false, type: "string", documentation: "Bucket" }
        Stage: { required: true, type: "string", default: "dev", documentation: "Stage" }
    }
    conditions: [
        { fn: "isSet", argv: [{ ref: "Endpoint" }] }
        { fn: "parseURL", argv: [{ ref: "Endpoint" }], assign: "url" }
        { fn: "booleanEquals", argv: [{ ref: "UseFips" }, true] }
        { fn: "isSet", argv: [{ ref: "Bucket" }] }
        { fn: "substring", argv: [{ ref: "Bucket" }, 0, 3, false], assign: "prefix" }
        { fn: "stringEquals", argv: [{ ref: "prefix" }, "abc"] }
        { fn: "uriEncode", argv: [{ ref: "Bucket" }], assign: "enc" }
        { fn: "isValidHostLabel", argv: [{ ref: "Region" }, false] }
        { fn: "not", argv: [{ fn: "stringEquals", argv: [{ ref: "Stage" }, "beta"] }] }
        { fn: "getAttr", argv: [{ ref: "url" }, "scheme"], assign: "scheme" }
        { fn: "stringEquals", argv: [{ ref: "Stage" }, "prod"] }
    ]
    results: [
        {
            conditions: []
            endpoint: {
                url: "https://{enc}.{Region}.example.com"
                properties: {
                    authSchemes: [{ name: "sigv4", signingRegion: "{Region}" }]
                }
                headers: { "x-bucket-prefix": ["{prefix}"] }
            }
            type: "endpoint"
        }
        {
            conditions: []
            endpoint: {
                url: "{url#scheme}://{url#authority}/custom"
                properties: {}
                headers: {}
            }
            type: "endpoint"
        }
        {
            conditions: []
            error: "'{Stage}' is not a valid stage."
            type: "error"
        }
        {
            conditions: []
            endpoint: {
                url: "https://fips.{Region}.example.com"
                properties: {}
                headers: {}
            }
            type: "endpoint"
        }
        {
            conditions: []
            endpoint: {
                url: "https://{Region}.example.com"
                properties: {}
                headers: {}
            }
            type: "endpoint"
        }
    ]
    root: 2
    nodeCount: 10
    nodes: "/////wAAAAH/////AAAAAAAAAAMAAAAEAAAAAQX14QL/////AAAAAgX14QQAAAAFAAAAAwAAAAYAAAAIAAAABAAAAAf/////AAAABQAAAAr/////AAAACAX14QUAAAAJAAAACgX14QUF9eEDAAAABgX14QH/////"
})
@restJson1
service BddRichService {
    version: "2024-01-01"
    operations: [Ping]
}

@http(method: "GET", uri: "/ping")
operation Ping {}
