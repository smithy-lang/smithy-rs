$version: "1.0"

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
    version: "2021-12-01",
    resources: [PokemonSpecies, Storage],
    operations: [
        GetServerStatistics,
        DoNothing,
        CapturePokemon,
        CheckHealth,
        StreamPokemonRadio
    ],
}

/// A users current Pokémon storage.
resource Storage {
    identifiers: {
        user: String
    },
    read: GetStorage,
}

/// Retrieve information about your Pokédex.
@readonly
@http(uri: "/pokedex/{user}", method: "GET")
operation GetStorage {
    input: GetStorageInput,
    output: GetStorageOutput,
    errors: [ResourceNotFoundException, StorageAccessNotAuthorized, ValidationException],
}

/// Not authorized to access Pokémon storage.
@error("client")
@httpError(401)
structure StorageAccessNotAuthorized {}

/// A request to access Pokémon storage.
@input
@sensitive
structure GetStorageInput {
    @required
    @httpLabel
    user: String,
    @required
    @httpHeader("passcode")
    passcode: String,
}

/// A list of Pokémon species.
list SpeciesCollection {
    member: String
}

/// Contents of the Pokémon storage.
@output
structure GetStorageOutput {
    @required
    collection: SpeciesCollection
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

/// Fetch a radio song from the database and stream it back as a playable audio.
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
