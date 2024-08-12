// TODO(https://github.com/smithy-lang/smithy-rs/issues/1508)
// reconcile this model with the main one living inside codegen-server-test/model/pokemon.smithy
// once the Typescript implementation supports Streaming and Union shapes.
$version: "1.0"

namespace com.aws.example.ts

use aws.protocols#restJson1
use com.aws.example#CheckHealth
use com.aws.example#DoNothing
use com.aws.example#GetServerStatistics
use com.aws.example#PokemonSpecies

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01"
    resources: [
        PokemonSpecies
    ]
    operations: [
        GetServerStatistics
        DoNothing
        CheckHealth
    ]
}
