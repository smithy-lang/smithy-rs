$version: "2"

namespace com.aws.example

use aws.protocols#awsJson1_1
use smithy.framework#ValidationException
use com.aws.example#GetServerStatistics
use com.aws.example#DoNothing
use com.aws.example#CheckHealth

/// The Pokemon Service allows you to retrieve information about Pokemon species.
@title("Pokemon Service")
@awsJson1_1
service PokemonService {
    version: "2024-03-18"
    operations: [
        GetServerStatistics
        DoNothing
        CapturePokemon
        CheckHealth
    ]
}

/// Capture Pokemon via event streams.
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
