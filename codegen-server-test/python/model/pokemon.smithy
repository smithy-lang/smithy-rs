/// TODO(https://github.com/awslabs/smithy-rs/issues/1508)
/// $econcile this model with the main one living inside codegen-server-test/model/pokemon.smithy
/// once the Python implementation supports Streaming and Union shapes.
$version: "1.0"

namespace com.aws.example.python

use aws.protocols#restJson1
use com.aws.example#PokemonSpecies
use com.aws.example#Storage
use com.aws.example#GetServerStatistics
use com.aws.example#DoNothing
use com.aws.example#CheckHealth
use com.aws.example#FlavorText


/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01",
    resources: [PokemonSpecies],
    operations: [
        GetServerStatistics,
        DoNothing,
        CheckHealth,
        StreamPokemonRadio,
        GetUnion
    ],
}

/// Fetch the radio song from the database and stream it back as a playable audio.
@readonly
@http(uri: "/radio", method: "GET")
operation StreamPokemonRadio {
    output: StreamPokemonRadioOutput,
}

@output
structure StreamPokemonRadioOutput {
    @httpPayload
    data: StreamingBlob,
}

@streaming
blob StreamingBlob

@readonly
@http(uri: "/union", method: "GET")
operation GetUnion {
    output: GetUnionOutput,
}

@output
structure GetUnionOutput {
    @required
    data: MyUnion
}

union MyUnion {
    integer: Integer
    @length(min: 1, max: 100)
    string: String
    something: FlavorText
}
