$version: "2"

// A CBOR (rpcv2Cbor) variant of the Pokémon service model, used to generate a
// client that speaks RpcV2Cbor against the multi-protocol Pokémon server (which
// supports both restJson1 and rpcv2Cbor). Mirrors `pokemon-awsjson.smithy`.

namespace com.aws.example

use smithy.protocols#rpcv2Cbor
use smithy.framework#ValidationException
use com.aws.example#GetServerStatistics
use com.aws.example#DoNothing
use com.aws.example#CheckHealth

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@rpcv2Cbor
service PokemonService {
    version: "2024-03-18"
    operations: [
        GetServerStatistics
        DoNothing
        CheckHealth
    ]
}
