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
use smithy.framework#ValidationException


/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01"
    resources: [PokemonSpecies]
    operations: [
        GetServerStatistics
        DoNothing
        CapturePokemon
        CheckHealth
        StreamPokemonRadio
    ],
}

/// Capture Pokémons via event streams.
@http(uri: "/capture-pokemon-event/{region}", method: "POST")
operation CapturePokemon {
    input: CapturePokemonEventsInput,
    output: CapturePokemonEventsOutput,
    errors: [UnsupportedRegionError, ThrottlingError, ValidationException]
}

@input
structure CapturePokemonEventsInput {
    @httpPayload
    events: AttemptCapturingPokemonEvent,

    @httpLabel
    @required
    region: String,
}

@output
structure CapturePokemonEventsOutput {
    @httpPayload
    events: CapturePokemonEvents,
}

@streaming
union AttemptCapturingPokemonEvent {
    event: CapturingEvent,
    masterball_unsuccessful: MasterBallUnsuccessful,
}

structure CapturingEvent {
    @eventPayload
    payload: CapturingPayload,
}

structure CapturingPayload {
    name: String,
    pokeball: String,
}

@streaming
union CapturePokemonEvents {
    event: CaptureEvent,
    invalid_pokeball: InvalidPokeballError,
    throttlingError: ThrottlingError,
}

structure CaptureEvent {
    @eventHeader
    name: String,
    @eventHeader
    captured: Boolean,
    @eventHeader
    shiny: Boolean,
    @eventPayload
    pokedex_update: Blob,
}

@error("server")
structure UnsupportedRegionError {
    @required
    region: String,
}

@error("client")
structure InvalidPokeballError {
    @required
    pokeball: String,
}
@error("server")
structure MasterBallUnsuccessful {
    message: String,
}

@error("client")
structure ThrottlingError {}

/// Fetch the radio song from the database and stream it back as a playable audio.
@readonly
@http(uri: "/radio", method: "GET")
operation StreamPokemonRadio {
    output: StreamPokemonRadioOutput
}

@output
structure StreamPokemonRadioOutput {
    @httpPayload
    data: StreamingBlob
}

@streaming
blob StreamingBlob
