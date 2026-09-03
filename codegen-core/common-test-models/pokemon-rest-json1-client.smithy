$version: "2"

namespace com.aws.example.multi

use aws.protocols#restJson1
use com.aws.example#CheckHealth
use com.aws.example#DoNothing
use com.aws.example#GetServerStatistics
use com.aws.example#PokemonSpecies
use com.aws.example#Storage
use com.aws.example#StreamPokemonRadio

@restJson1
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
