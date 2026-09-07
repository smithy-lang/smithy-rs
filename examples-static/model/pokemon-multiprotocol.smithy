$version: "2"

namespace com.aws.example

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restJson1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor
use smithy.api#http

/// Minimal Pokemon service model used for protocol stack benchmarks.
@title("Pokemon Service")
@restJson1
@restXml
@awsJson1_0
@awsJson1_1
@rpcv2Cbor
service PokemonService {
    version: "2024-03-18"
    operations: [
        GetServerStatistics
    ]
}

/// Retrieve HTTP server statistics, such as calls count.
@readonly
@http(uri: "/stats", method: "GET")
operation GetServerStatistics {
    input := {}
    output := {
        /// The number of calls executed by the server.
        @required
        calls_count: Long
    }
}
