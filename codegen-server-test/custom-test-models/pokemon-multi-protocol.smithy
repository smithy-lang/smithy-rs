$version: "2"

namespace com.aws.example.multi

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restJson1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor
use com.aws.example#CheckHealth
use com.aws.example#DoNothing
use com.aws.example#GetServerStatistics
use com.aws.example#PokemonSpecies
use com.aws.example#Storage
use com.aws.example#StreamPokemonRadio

// This service is the all-protocol inspection fixture. It reuses the Pokemon
// model's operations that are supported by every selected protocol.
//
// CapturePokemon is intentionally excluded because the current AWS JSON server
// generator cannot bind its event-stream payload shape.
@awsJson1_0
@awsJson1_1
@restJson1
@restXml
@rpcv2Cbor
service PokemonService {
    version: "2024-03-18"
    resources: [
        PokemonSpecies
        Storage
    ]
    operations: [
        GetServerStatistics
        DoNothing
        CheckHealth
        StreamPokemonRadio
    ]
}
