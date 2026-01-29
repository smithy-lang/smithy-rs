$version: "2"

namespace com.aws.example

use aws.protocols#restJson1
use smithy.framework#ValidationException
use com.aws.example#PokemonSpecies
use com.aws.example#GetServerStatistics
use com.aws.example#DoNothing
use com.aws.example#CheckHealth
use com.aws.example#ResourceNotFoundException

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
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
        CapturePokemon
        CheckHealth
        StreamPokemonRadio
    ]
}

/// A users current Pokémon storage.
resource Storage {
    identifiers: {
        user: String
    }
    read: GetStorage
}

/// Retrieve information about your Pokédex.
@readonly
@http(uri: "/pokedex/{user}", method: "GET")
operation GetStorage {
    input := @sensitive @documentation("A request to access Pokémon storage.") {
        @required
        @httpLabel
        user: String
        @required
        @httpHeader("passcode")
        passcode: String
    }
    output := @documentation("Contents of the Pokémon storage.") {
        @required
        collection: SpeciesCollection
    }
    errors: [
        ResourceNotFoundException
        StorageAccessNotAuthorized
        ValidationException
    ]
}

/// Not authorized to access Pokémon storage.
@error("client")
@httpError(401)
structure StorageAccessNotAuthorized {}

/// A list of Pokémon species.
list SpeciesCollection {
    member: String
}

/// Capture Pokémons via event streams.
@http(uri: "/capture-pokemon-event/{region}", method: "POST")
operation CapturePokemon {
    input := {
        @httpPayload
        events: AttemptCapturingPokemonEvent

        @httpLabel
        @required
        region: String
    }
    output := {
        @httpPayload
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

structure CapturingPayload for PokemonSpecies {
    $name
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

/// Fetch a radio song from the database and stream it back as a playable audio.
@readonly
@http(uri: "/radio", method: "GET")
operation StreamPokemonRadio {
    output := {
        @httpPayload
        data: StreamingBlob = ""
    }
}

@streaming
blob StreamingBlob
