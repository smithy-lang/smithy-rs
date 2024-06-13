$version: "2"

// TODO(https://github.com/smithy-lang/smithy-rs/issues/2215)
// This is a temporary model to test AwsJson 1.0 with @streaming.
// This model will be removed when protocol tests support @streaming.

namespace com.aws.example

use aws.protocols#awsJson1_0
use smithy.framework#ValidationException
use com.aws.example#PokemonSpecies
use com.aws.example#GetServerStatistics
use com.aws.example#DoNothing
use com.aws.example#CheckHealth

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@awsJson1_0
service PokemonService {
    version: "2024-03-18"
    operations: [
        GetServerStatistics
        DoNothing
        CapturePokemon
        CheckHealth
    ]
}

/// Capture Pokémons via event streams.
@http(uri: "/simple-struct-operation", method: "POST")
operation CapturePokemon {
    input := {
        events: AttemptCapturingPokemonEvent
    }
    output := {
        events: CapturePokemonEvents
    }
    errors: [
        UnsupportedRegionError
        ThrottlingError
        ValidationException
    ]
}

@streaming
union AttemptCapturingPokemonEvent {
    event: CapturingEvent
    masterball_unsuccessful: MasterBallUnsuccessful
}

structure CapturingEvent {
    @eventPayload
    payload: CapturingPayload
}

structure CapturingPayload {
    name: String
    pokeball: String
}

@streaming
union CapturePokemonEvents {
    event: CaptureEvent
    invalid_pokeball: InvalidPokeballError
    throttlingError: ThrottlingError
}

structure CaptureEvent {
    @eventHeader
    name: String
    @eventHeader
    captured: Boolean
    @eventHeader
    shiny: Boolean
    @eventPayload
    pokedex_update: Blob
}

@error("server")
structure UnsupportedRegionError {
    @required
    region: String
}

@error("client")
structure InvalidPokeballError {
    @required
    pokeball: String
}
@error("server")
structure MasterBallUnsuccessful {
    message: String
}

@error("client")
structure ThrottlingError {}
