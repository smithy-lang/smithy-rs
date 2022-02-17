$version: "1.0"

namespace com.aws.example

use aws.protocols#restJson1

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01",
    resources: [PokemonSpecies],
    operations: [GetServerStatistics],
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
    input: GetPokemonInput,
    output: GetPokemonOutput,
    errors: [ResourceNotFoundException],
}

structure GetPokemonInput {
    @required
    @httpLabel
    name: String
}

structure GetPokemonOutput {
    /// The name for this resource.
    name: String,

    /// A list of flavor text entries for this Pokémon species.
    flavorTextEntries: FlavorTextEntries
}

/// Retrieve HTTP server statistiscs, such as calls count.
@readonly
@http(uri: "/stats", method: "GET")
operation GetServerStatistics {
    input: GetServerStatisticsInput,
    output: GetServerStatisticsOutput,
}

structure GetServerStatisticsInput { }

structure GetServerStatisticsOutput {
    /// The number of calls executed by the server.
    calls_count: Long,
}

list FlavorTextEntries {
    member: FlavorText
}

structure FlavorText {
    /// The localized flavor text for an API resource in a specific language.
    flavorText: String,

    /// The language this name is in.
    language: String,
}

@error("client")
@httpError(404)
structure ResourceNotFoundException {
    message: String,
}
