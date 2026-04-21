$version: "2"

namespace com.aws.example

use smithy.framework#ValidationException

/// A Pokémon species forms the basis for at least one Pokémon.
@title("Pokémon Species")
resource PokemonSpecies {
    identifiers: {
        name: String
    }
    read: GetPokemonSpecies
}

/// Retrieve information about a Pokémon species.
@readonly
@http(uri: "/pokemon-species/{name}", method: "GET")
operation GetPokemonSpecies {
    input := {
        @required
        @httpLabel
        name: String
    }
    output := {
        /// The name for this resource.
        @required
        name: String

        /// A list of flavor text entries for this Pokémon species.
        @required
        flavorTextEntries: FlavorTextEntries
    }
    errors: [
        ResourceNotFoundException
        ValidationException
    ]
}

/// Retrieve HTTP server statistiscs, such as calls count.
@readonly
@http(uri: "/stats", method: "GET")
operation GetServerStatistics {
    input := {}
    output := {
        /// The number of calls executed by the server.
        @required
        calls_count: Long
    }
}

list FlavorTextEntries {
    member: FlavorText
}

structure FlavorText {
    /// The localized flavor text for an API resource in a specific language.
    @required
    flavorText: String

    /// The language this name is in.
    @required
    language: Language
}

/// Supported languages for FlavorText entries.
enum Language {
    /// American English.
    ENGLISH = "en"
    /// Español.
    SPANISH = "es"
    /// Italiano.
    ITALIAN = "it"
    /// 日本語。
    JAPANESE = "jp"
}

/// DoNothing operation, used to stress test the framework.
@readonly
@http(uri: "/do-nothing", method: "GET")
operation DoNothing {
    input := {}
    output := {}
}

/// Health check operation, to check the service is up
/// Not yet a deep check
@readonly
@http(uri: "/ping", method: "GET")
operation CheckHealth {
}

@error("client")
@httpError(404)
structure ResourceNotFoundException {
    @required
    message: String
}
