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
use smithy.framework#ValidationException


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
@http(uri: "/union", method: "POST")
operation GetUnion {
    input: GetUnionInput,
    output: GetUnionOutput,
    errors: [ValidationException]
}

@input
structure GetUnionInput {
    @required
    data: MyUnion
}
@output
structure GetUnionOutput {
    @required
    data: MyUnion
}

structure ConstrainedStuff {
    //@length(min: 1, max: 100)
    inner: String
}

union InnerUnion {
    a: Integer
    b: ConstrainedStuff
}

union MyUnion {
    integer: Integer
    string: String
    something: FlavorText
    string2: String
    inner: InnerUnion
}
