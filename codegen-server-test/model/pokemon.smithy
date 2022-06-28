$version: "1.0"

namespace com.aws.example

use aws.protocols#restJson1

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01",
    resources: [PokemonSpecies],
    operations: [GetServerStatistics, EmptyOperation],
}

/// A Pokémon species forms the basis for at least one Pokémon.
@title("Pokémon Species")
resource PokemonSpecies {
    identifiers: {
        name: String
    },
    read: GetPokemonSpecies,
}

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

@output
structure GetPokemonSpeciesOutput {
    /// The name for this resource.
    @required
    name: String,

    /// A list of flavor text entries for this Pokémon species.
    @required
    flavorTextEntries: FlavorTextEntries
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

@error("client")
@httpError(404)
structure ResourceNotFoundException {
    @required
    message: String,
}
