// coverage-service: test.bdd#BddService
// coverage-plugins: client
$version: "2.0"

namespace test.bdd

use aws.protocols#restJson1
use smithy.rules#clientContextParams
use smithy.rules#endpointBdd

@clientContextParams(
    Stage: { type: "string", documentation: "Deployment stage" }
)
@endpointBdd({
    version: "1.1"
    parameters: {
        Stage: { required: false, documentation: "Deployment stage", type: "string" }
    }
    conditions: [
        { fn: "isSet", argv: [{ ref: "Stage" }] }
    ]
    results: [
        {
            conditions: []
            error: "'{Stage}' is not a valid stage."
            type: "error"
        }
        {
            conditions: []
            endpoint: {
                url: "https://prod.example.com"
                properties: {}
                headers: {}
            }
            type: "endpoint"
        }
    ]
    root: 2
    nodeCount: 2
    nodes: "/////wAAAAH/////AAAAAAX14QEF9eEC"
})
@restJson1
service BddService {
    version: "2022-01-01"
    operations: [Ping]
}

@http(method: "GET", uri: "/ping")
operation Ping {}
