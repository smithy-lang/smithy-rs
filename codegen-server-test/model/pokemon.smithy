$version: "1.0"

namespace com.aws.example

use aws.protocols#restJson1

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01",
    resources: [PokemonSpecies, Storage],
    operations: [GetServerStatistics, EmptyOperation, CapturePokemonOperation, HealthCheckOperation],
}

/// A Pokémon species forms the basis for at least one Pokémon.
@title("Pokémon Species")
resource PokemonSpecies {
    identifiers: {
        name: String
    },
    read: GetPokemonSpecies,
}

/// A users current Pokémon storage.
resource Storage {
    identifiers: {
        user: String
    },
    read: GetStorage,
}

/// Capture Pokémons via event streams
@http(uri: "/capture-pokemon-event/{region}", method: "POST")
operation CapturePokemonOperation {
    input: CapturePokemonOperationEventsInput,
    output: CapturePokemonOperationEventsOutput,
    errors: [UnsupportedRegionError, ThrottlingError]
}

@input
structure CapturePokemonOperationEventsInput {
    @httpPayload
    events: AttemptCapturingPokemonEvent,

    @httpLabel
    @required
    region: String,
}

@output
structure CapturePokemonOperationEventsOutput {
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
    @required
    message: String,
}
@error("client")
structure ThrottlingError {}

/// Retrieve information about a Pokémon species.
@readonly
@http(uri: "/pokemon-species/{name}", method: "GET")
operation GetPokemonSpecies {
    input: GetPokemonSpeciesInput,
    output: GetPokemonSpeciesOutput,
    errors: [ResourceNotFoundException],
}

@input
structure GetPokemonSpeciesInput {
    @required
    @httpLabel
    name: String
}

structure GetPokemonSpeciesOutput {
    /// The name for this resource.
    @required
    name: String,

    /// A list of flavor text entries for this Pokémon species.
    @required
    flavorTextEntries: FlavorTextEntries
}

/// Retrieve information about your Pokedex.
@readonly
@http(uri: "/pokedex/{user}", method: "GET")
operation GetStorage {
    input: GetStorageInput,
    output: GetStorageOutput,
    errors: [ResourceNotFoundException, NotAuthorized],
}

/// Not authorized to access Pokémon storage.
@error("client")
@httpError(401)
structure NotAuthorized {}

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
    member: GetPokemonSpeciesOutput
}

/// Contents of the Pokémon storage.
@output
structure GetStorageOutput {
    @required
    collection: SpeciesCollection
}

/// Retrieve HTTP server statistiscs, such as calls count.
@readonly
@http(uri: "/stats", method: "GET")
operation GetServerStatistics {
    input: GetServerStatisticsInput,
    output: GetServerStatisticsOutput,
}

@input
structure GetServerStatisticsInput { }

@output
structure GetServerStatisticsOutput {
    /// The number of calls executed by the server.
    @required
    calls_count: Long,
}

list FlavorTextEntries {
    member: FlavorText
}

structure FlavorText {
    /// The localized flavor text for an API resource in a specific language.
    @required
    flavorText: String,

    /// The language this name is in.
    @required
    language: Language,
}

/// Supported languages for FlavorText entries.
@enum([
    {
        name: "ENGLISH",
        value: "en",
        documentation: "American English.",
    },
    {
        name: "SPANISH",
        value: "es",
        documentation: "Español.",
    },
    {
        name: "ITALIAN",
        value: "it",
        documentation: "Italiano.",
    },
    {
        name: "JAPANESE",
        value: "jp",
        documentation: "日本語。",
    },
])
string Language

/// Empty operation, used to stress test the framework.
@readonly
@http(uri: "/empty-operation", method: "GET")
operation EmptyOperation {
    input: EmptyOperationInput,
    output: EmptyOperationOutput,
}

@input
structure EmptyOperationInput { }

@output
structure EmptyOperationOutput { }

/// Health check operation, to check the service is up
@readonly
@http(uri: "/ping", method: "GET")
operation HealthCheckOperation {
    input: HealthCheckOperationInput,
    output: HealthCheckOperationOutput,
}

@input
structure HealthCheckOperationInput { }

@output
structure HealthCheckOperationOutput { }

@error("client")
@httpError(404)
structure ResourceNotFoundException {
    @required
    message: String,
}
