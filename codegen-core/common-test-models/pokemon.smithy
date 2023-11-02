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
        StreamPokemonRadio,
CustomOp0,
CustomOp1,
CustomOp2,
CustomOp3,
CustomOp4,
CustomOp5,
CustomOp6,
CustomOp7,
CustomOp8,
CustomOp9,
CustomOp10,
CustomOp11,
CustomOp12,
CustomOp13,
CustomOp14,
CustomOp15,
CustomOp16,
CustomOp17,
CustomOp18,
CustomOp19,
CustomOp20,
CustomOp21,
CustomOp22,
CustomOp23,
CustomOp24,
CustomOp25,
CustomOp26,
CustomOp27,
CustomOp28,
CustomOp29,
CustomOp30,
CustomOp31,
CustomOp32,
CustomOp33,
CustomOp34,
CustomOp35,
CustomOp36,
CustomOp37,
CustomOp38,
CustomOp39,
CustomOp40,
CustomOp41,
CustomOp42,
CustomOp43,
CustomOp44,
CustomOp45,
CustomOp46,
CustomOp47,
CustomOp48,
CustomOp49,
CustomOp50,
CustomOp51,
CustomOp52,
CustomOp53,
CustomOp54,
CustomOp55,
CustomOp56,
CustomOp57,
CustomOp58,
CustomOp59,
CustomOp60,
CustomOp61,
CustomOp62,
CustomOp63,
CustomOp64,
CustomOp65,
CustomOp66,
CustomOp67,
CustomOp68,
CustomOp69,
CustomOp70,
CustomOp71,
CustomOp72,
CustomOp73,
CustomOp74,
CustomOp75,
CustomOp76,
CustomOp77,
CustomOp78,
CustomOp79,
CustomOp80,
CustomOp81,
CustomOp82,
CustomOp83,
CustomOp84,
CustomOp85,
CustomOp86,
CustomOp87,
CustomOp88,
CustomOp89,
CustomOp90,
CustomOp91,
CustomOp92,
CustomOp93,
CustomOp94,
CustomOp95,
CustomOp96,
CustomOp97,
CustomOp98,
CustomOp99,
CustomOp100,
CustomOp101,
CustomOp102,
CustomOp103,
CustomOp104,
CustomOp105,
CustomOp106,
CustomOp107,
CustomOp108,
CustomOp109,
CustomOp110,
CustomOp111,
CustomOp112,
CustomOp113,
CustomOp114,
CustomOp115,
CustomOp116,
CustomOp117,
CustomOp118,
CustomOp119,
CustomOp120,
CustomOp121,
CustomOp122,
CustomOp123,
CustomOp124,
CustomOp125,
CustomOp126,
CustomOp127,
CustomOp128,
CustomOp129,
CustomOp130,
CustomOp131,
CustomOp132,
CustomOp133,
CustomOp134,
CustomOp135,
CustomOp136,
CustomOp137,
CustomOp138,
CustomOp139,
CustomOp140,
CustomOp141,
CustomOp142,
CustomOp143,
CustomOp144,
CustomOp145,
CustomOp146,
CustomOp147,
CustomOp148,
CustomOp149,
CustomOp150,
CustomOp151,
CustomOp152,
CustomOp153,
CustomOp154,
CustomOp155,
CustomOp156,
CustomOp157,
CustomOp158,
CustomOp159,
CustomOp160,
CustomOp161,
CustomOp162,
CustomOp163,
CustomOp164,
CustomOp165,
CustomOp166,
CustomOp167,
CustomOp168,
CustomOp169,
CustomOp170,
CustomOp171,
CustomOp172,
CustomOp173,
CustomOp174,
CustomOp175,
CustomOp176,
CustomOp177,
CustomOp178,
CustomOp179,
CustomOp180,
CustomOp181,
CustomOp182,
CustomOp183,
CustomOp184,
CustomOp185,
CustomOp186,
CustomOp187,
CustomOp188,
CustomOp189,
CustomOp190,
CustomOp191,
CustomOp192,
CustomOp193,
CustomOp194,
CustomOp195,
CustomOp196,
CustomOp197,
CustomOp198,
CustomOp199
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

/// -----------------------

    @http(uri: "/testhandler0/{region}", method: "POST")
    operation CustomOp0 {
        input: InputFor0,
        output: OutputFor0,
        errors: [ValidationException]
    }

    @input
    structure InputFor0 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor0 {
        @required
        output: String,
    }


    @http(uri: "/testhandler1/{region}", method: "POST")
    operation CustomOp1 {
        input: InputFor1,
        output: OutputFor1,
        errors: [ValidationException]
    }

    @input
    structure InputFor1 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor1 {
        @required
        output: String,
    }


    @http(uri: "/testhandler2/{region}", method: "POST")
    operation CustomOp2 {
        input: InputFor2,
        output: OutputFor2,
        errors: [ValidationException]
    }

    @input
    structure InputFor2 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor2 {
        @required
        output: String,
    }


    @http(uri: "/testhandler3/{region}", method: "POST")
    operation CustomOp3 {
        input: InputFor3,
        output: OutputFor3,
        errors: [ValidationException]
    }

    @input
    structure InputFor3 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor3 {
        @required
        output: String,
    }


    @http(uri: "/testhandler4/{region}", method: "POST")
    operation CustomOp4 {
        input: InputFor4,
        output: OutputFor4,
        errors: [ValidationException]
    }

    @input
    structure InputFor4 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor4 {
        @required
        output: String,
    }


    @http(uri: "/testhandler5/{region}", method: "POST")
    operation CustomOp5 {
        input: InputFor5,
        output: OutputFor5,
        errors: [ValidationException]
    }

    @input
    structure InputFor5 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor5 {
        @required
        output: String,
    }


    @http(uri: "/testhandler6/{region}", method: "POST")
    operation CustomOp6 {
        input: InputFor6,
        output: OutputFor6,
        errors: [ValidationException]
    }

    @input
    structure InputFor6 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor6 {
        @required
        output: String,
    }


    @http(uri: "/testhandler7/{region}", method: "POST")
    operation CustomOp7 {
        input: InputFor7,
        output: OutputFor7,
        errors: [ValidationException]
    }

    @input
    structure InputFor7 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor7 {
        @required
        output: String,
    }


    @http(uri: "/testhandler8/{region}", method: "POST")
    operation CustomOp8 {
        input: InputFor8,
        output: OutputFor8,
        errors: [ValidationException]
    }

    @input
    structure InputFor8 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor8 {
        @required
        output: String,
    }


    @http(uri: "/testhandler9/{region}", method: "POST")
    operation CustomOp9 {
        input: InputFor9,
        output: OutputFor9,
        errors: [ValidationException]
    }

    @input
    structure InputFor9 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor9 {
        @required
        output: String,
    }


    @http(uri: "/testhandler10/{region}", method: "POST")
    operation CustomOp10 {
        input: InputFor10,
        output: OutputFor10,
        errors: [ValidationException]
    }

    @input
    structure InputFor10 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor10 {
        @required
        output: String,
    }


    @http(uri: "/testhandler11/{region}", method: "POST")
    operation CustomOp11 {
        input: InputFor11,
        output: OutputFor11,
        errors: [ValidationException]
    }

    @input
    structure InputFor11 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor11 {
        @required
        output: String,
    }


    @http(uri: "/testhandler12/{region}", method: "POST")
    operation CustomOp12 {
        input: InputFor12,
        output: OutputFor12,
        errors: [ValidationException]
    }

    @input
    structure InputFor12 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor12 {
        @required
        output: String,
    }


    @http(uri: "/testhandler13/{region}", method: "POST")
    operation CustomOp13 {
        input: InputFor13,
        output: OutputFor13,
        errors: [ValidationException]
    }

    @input
    structure InputFor13 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor13 {
        @required
        output: String,
    }


    @http(uri: "/testhandler14/{region}", method: "POST")
    operation CustomOp14 {
        input: InputFor14,
        output: OutputFor14,
        errors: [ValidationException]
    }

    @input
    structure InputFor14 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor14 {
        @required
        output: String,
    }


    @http(uri: "/testhandler15/{region}", method: "POST")
    operation CustomOp15 {
        input: InputFor15,
        output: OutputFor15,
        errors: [ValidationException]
    }

    @input
    structure InputFor15 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor15 {
        @required
        output: String,
    }


    @http(uri: "/testhandler16/{region}", method: "POST")
    operation CustomOp16 {
        input: InputFor16,
        output: OutputFor16,
        errors: [ValidationException]
    }

    @input
    structure InputFor16 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor16 {
        @required
        output: String,
    }


    @http(uri: "/testhandler17/{region}", method: "POST")
    operation CustomOp17 {
        input: InputFor17,
        output: OutputFor17,
        errors: [ValidationException]
    }

    @input
    structure InputFor17 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor17 {
        @required
        output: String,
    }


    @http(uri: "/testhandler18/{region}", method: "POST")
    operation CustomOp18 {
        input: InputFor18,
        output: OutputFor18,
        errors: [ValidationException]
    }

    @input
    structure InputFor18 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor18 {
        @required
        output: String,
    }


    @http(uri: "/testhandler19/{region}", method: "POST")
    operation CustomOp19 {
        input: InputFor19,
        output: OutputFor19,
        errors: [ValidationException]
    }

    @input
    structure InputFor19 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor19 {
        @required
        output: String,
    }


    @http(uri: "/testhandler20/{region}", method: "POST")
    operation CustomOp20 {
        input: InputFor20,
        output: OutputFor20,
        errors: [ValidationException]
    }

    @input
    structure InputFor20 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor20 {
        @required
        output: String,
    }


    @http(uri: "/testhandler21/{region}", method: "POST")
    operation CustomOp21 {
        input: InputFor21,
        output: OutputFor21,
        errors: [ValidationException]
    }

    @input
    structure InputFor21 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor21 {
        @required
        output: String,
    }


    @http(uri: "/testhandler22/{region}", method: "POST")
    operation CustomOp22 {
        input: InputFor22,
        output: OutputFor22,
        errors: [ValidationException]
    }

    @input
    structure InputFor22 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor22 {
        @required
        output: String,
    }


    @http(uri: "/testhandler23/{region}", method: "POST")
    operation CustomOp23 {
        input: InputFor23,
        output: OutputFor23,
        errors: [ValidationException]
    }

    @input
    structure InputFor23 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor23 {
        @required
        output: String,
    }


    @http(uri: "/testhandler24/{region}", method: "POST")
    operation CustomOp24 {
        input: InputFor24,
        output: OutputFor24,
        errors: [ValidationException]
    }

    @input
    structure InputFor24 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor24 {
        @required
        output: String,
    }


    @http(uri: "/testhandler25/{region}", method: "POST")
    operation CustomOp25 {
        input: InputFor25,
        output: OutputFor25,
        errors: [ValidationException]
    }

    @input
    structure InputFor25 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor25 {
        @required
        output: String,
    }


    @http(uri: "/testhandler26/{region}", method: "POST")
    operation CustomOp26 {
        input: InputFor26,
        output: OutputFor26,
        errors: [ValidationException]
    }

    @input
    structure InputFor26 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor26 {
        @required
        output: String,
    }


    @http(uri: "/testhandler27/{region}", method: "POST")
    operation CustomOp27 {
        input: InputFor27,
        output: OutputFor27,
        errors: [ValidationException]
    }

    @input
    structure InputFor27 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor27 {
        @required
        output: String,
    }


    @http(uri: "/testhandler28/{region}", method: "POST")
    operation CustomOp28 {
        input: InputFor28,
        output: OutputFor28,
        errors: [ValidationException]
    }

    @input
    structure InputFor28 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor28 {
        @required
        output: String,
    }


    @http(uri: "/testhandler29/{region}", method: "POST")
    operation CustomOp29 {
        input: InputFor29,
        output: OutputFor29,
        errors: [ValidationException]
    }

    @input
    structure InputFor29 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor29 {
        @required
        output: String,
    }


    @http(uri: "/testhandler30/{region}", method: "POST")
    operation CustomOp30 {
        input: InputFor30,
        output: OutputFor30,
        errors: [ValidationException]
    }

    @input
    structure InputFor30 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor30 {
        @required
        output: String,
    }


    @http(uri: "/testhandler31/{region}", method: "POST")
    operation CustomOp31 {
        input: InputFor31,
        output: OutputFor31,
        errors: [ValidationException]
    }

    @input
    structure InputFor31 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor31 {
        @required
        output: String,
    }


    @http(uri: "/testhandler32/{region}", method: "POST")
    operation CustomOp32 {
        input: InputFor32,
        output: OutputFor32,
        errors: [ValidationException]
    }

    @input
    structure InputFor32 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor32 {
        @required
        output: String,
    }


    @http(uri: "/testhandler33/{region}", method: "POST")
    operation CustomOp33 {
        input: InputFor33,
        output: OutputFor33,
        errors: [ValidationException]
    }

    @input
    structure InputFor33 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor33 {
        @required
        output: String,
    }


    @http(uri: "/testhandler34/{region}", method: "POST")
    operation CustomOp34 {
        input: InputFor34,
        output: OutputFor34,
        errors: [ValidationException]
    }

    @input
    structure InputFor34 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor34 {
        @required
        output: String,
    }


    @http(uri: "/testhandler35/{region}", method: "POST")
    operation CustomOp35 {
        input: InputFor35,
        output: OutputFor35,
        errors: [ValidationException]
    }

    @input
    structure InputFor35 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor35 {
        @required
        output: String,
    }


    @http(uri: "/testhandler36/{region}", method: "POST")
    operation CustomOp36 {
        input: InputFor36,
        output: OutputFor36,
        errors: [ValidationException]
    }

    @input
    structure InputFor36 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor36 {
        @required
        output: String,
    }


    @http(uri: "/testhandler37/{region}", method: "POST")
    operation CustomOp37 {
        input: InputFor37,
        output: OutputFor37,
        errors: [ValidationException]
    }

    @input
    structure InputFor37 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor37 {
        @required
        output: String,
    }


    @http(uri: "/testhandler38/{region}", method: "POST")
    operation CustomOp38 {
        input: InputFor38,
        output: OutputFor38,
        errors: [ValidationException]
    }

    @input
    structure InputFor38 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor38 {
        @required
        output: String,
    }


    @http(uri: "/testhandler39/{region}", method: "POST")
    operation CustomOp39 {
        input: InputFor39,
        output: OutputFor39,
        errors: [ValidationException]
    }

    @input
    structure InputFor39 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor39 {
        @required
        output: String,
    }


    @http(uri: "/testhandler40/{region}", method: "POST")
    operation CustomOp40 {
        input: InputFor40,
        output: OutputFor40,
        errors: [ValidationException]
    }

    @input
    structure InputFor40 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor40 {
        @required
        output: String,
    }


    @http(uri: "/testhandler41/{region}", method: "POST")
    operation CustomOp41 {
        input: InputFor41,
        output: OutputFor41,
        errors: [ValidationException]
    }

    @input
    structure InputFor41 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor41 {
        @required
        output: String,
    }


    @http(uri: "/testhandler42/{region}", method: "POST")
    operation CustomOp42 {
        input: InputFor42,
        output: OutputFor42,
        errors: [ValidationException]
    }

    @input
    structure InputFor42 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor42 {
        @required
        output: String,
    }


    @http(uri: "/testhandler43/{region}", method: "POST")
    operation CustomOp43 {
        input: InputFor43,
        output: OutputFor43,
        errors: [ValidationException]
    }

    @input
    structure InputFor43 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor43 {
        @required
        output: String,
    }


    @http(uri: "/testhandler44/{region}", method: "POST")
    operation CustomOp44 {
        input: InputFor44,
        output: OutputFor44,
        errors: [ValidationException]
    }

    @input
    structure InputFor44 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor44 {
        @required
        output: String,
    }


    @http(uri: "/testhandler45/{region}", method: "POST")
    operation CustomOp45 {
        input: InputFor45,
        output: OutputFor45,
        errors: [ValidationException]
    }

    @input
    structure InputFor45 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor45 {
        @required
        output: String,
    }


    @http(uri: "/testhandler46/{region}", method: "POST")
    operation CustomOp46 {
        input: InputFor46,
        output: OutputFor46,
        errors: [ValidationException]
    }

    @input
    structure InputFor46 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor46 {
        @required
        output: String,
    }


    @http(uri: "/testhandler47/{region}", method: "POST")
    operation CustomOp47 {
        input: InputFor47,
        output: OutputFor47,
        errors: [ValidationException]
    }

    @input
    structure InputFor47 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor47 {
        @required
        output: String,
    }


    @http(uri: "/testhandler48/{region}", method: "POST")
    operation CustomOp48 {
        input: InputFor48,
        output: OutputFor48,
        errors: [ValidationException]
    }

    @input
    structure InputFor48 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor48 {
        @required
        output: String,
    }


    @http(uri: "/testhandler49/{region}", method: "POST")
    operation CustomOp49 {
        input: InputFor49,
        output: OutputFor49,
        errors: [ValidationException]
    }

    @input
    structure InputFor49 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor49 {
        @required
        output: String,
    }


    @http(uri: "/testhandler50/{region}", method: "POST")
    operation CustomOp50 {
        input: InputFor50,
        output: OutputFor50,
        errors: [ValidationException]
    }

    @input
    structure InputFor50 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor50 {
        @required
        output: String,
    }


    @http(uri: "/testhandler51/{region}", method: "POST")
    operation CustomOp51 {
        input: InputFor51,
        output: OutputFor51,
        errors: [ValidationException]
    }

    @input
    structure InputFor51 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor51 {
        @required
        output: String,
    }


    @http(uri: "/testhandler52/{region}", method: "POST")
    operation CustomOp52 {
        input: InputFor52,
        output: OutputFor52,
        errors: [ValidationException]
    }

    @input
    structure InputFor52 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor52 {
        @required
        output: String,
    }


    @http(uri: "/testhandler53/{region}", method: "POST")
    operation CustomOp53 {
        input: InputFor53,
        output: OutputFor53,
        errors: [ValidationException]
    }

    @input
    structure InputFor53 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor53 {
        @required
        output: String,
    }


    @http(uri: "/testhandler54/{region}", method: "POST")
    operation CustomOp54 {
        input: InputFor54,
        output: OutputFor54,
        errors: [ValidationException]
    }

    @input
    structure InputFor54 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor54 {
        @required
        output: String,
    }


    @http(uri: "/testhandler55/{region}", method: "POST")
    operation CustomOp55 {
        input: InputFor55,
        output: OutputFor55,
        errors: [ValidationException]
    }

    @input
    structure InputFor55 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor55 {
        @required
        output: String,
    }


    @http(uri: "/testhandler56/{region}", method: "POST")
    operation CustomOp56 {
        input: InputFor56,
        output: OutputFor56,
        errors: [ValidationException]
    }

    @input
    structure InputFor56 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor56 {
        @required
        output: String,
    }


    @http(uri: "/testhandler57/{region}", method: "POST")
    operation CustomOp57 {
        input: InputFor57,
        output: OutputFor57,
        errors: [ValidationException]
    }

    @input
    structure InputFor57 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor57 {
        @required
        output: String,
    }


    @http(uri: "/testhandler58/{region}", method: "POST")
    operation CustomOp58 {
        input: InputFor58,
        output: OutputFor58,
        errors: [ValidationException]
    }

    @input
    structure InputFor58 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor58 {
        @required
        output: String,
    }


    @http(uri: "/testhandler59/{region}", method: "POST")
    operation CustomOp59 {
        input: InputFor59,
        output: OutputFor59,
        errors: [ValidationException]
    }

    @input
    structure InputFor59 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor59 {
        @required
        output: String,
    }


    @http(uri: "/testhandler60/{region}", method: "POST")
    operation CustomOp60 {
        input: InputFor60,
        output: OutputFor60,
        errors: [ValidationException]
    }

    @input
    structure InputFor60 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor60 {
        @required
        output: String,
    }


    @http(uri: "/testhandler61/{region}", method: "POST")
    operation CustomOp61 {
        input: InputFor61,
        output: OutputFor61,
        errors: [ValidationException]
    }

    @input
    structure InputFor61 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor61 {
        @required
        output: String,
    }


    @http(uri: "/testhandler62/{region}", method: "POST")
    operation CustomOp62 {
        input: InputFor62,
        output: OutputFor62,
        errors: [ValidationException]
    }

    @input
    structure InputFor62 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor62 {
        @required
        output: String,
    }


    @http(uri: "/testhandler63/{region}", method: "POST")
    operation CustomOp63 {
        input: InputFor63,
        output: OutputFor63,
        errors: [ValidationException]
    }

    @input
    structure InputFor63 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor63 {
        @required
        output: String,
    }


    @http(uri: "/testhandler64/{region}", method: "POST")
    operation CustomOp64 {
        input: InputFor64,
        output: OutputFor64,
        errors: [ValidationException]
    }

    @input
    structure InputFor64 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor64 {
        @required
        output: String,
    }


    @http(uri: "/testhandler65/{region}", method: "POST")
    operation CustomOp65 {
        input: InputFor65,
        output: OutputFor65,
        errors: [ValidationException]
    }

    @input
    structure InputFor65 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor65 {
        @required
        output: String,
    }


    @http(uri: "/testhandler66/{region}", method: "POST")
    operation CustomOp66 {
        input: InputFor66,
        output: OutputFor66,
        errors: [ValidationException]
    }

    @input
    structure InputFor66 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor66 {
        @required
        output: String,
    }


    @http(uri: "/testhandler67/{region}", method: "POST")
    operation CustomOp67 {
        input: InputFor67,
        output: OutputFor67,
        errors: [ValidationException]
    }

    @input
    structure InputFor67 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor67 {
        @required
        output: String,
    }


    @http(uri: "/testhandler68/{region}", method: "POST")
    operation CustomOp68 {
        input: InputFor68,
        output: OutputFor68,
        errors: [ValidationException]
    }

    @input
    structure InputFor68 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor68 {
        @required
        output: String,
    }


    @http(uri: "/testhandler69/{region}", method: "POST")
    operation CustomOp69 {
        input: InputFor69,
        output: OutputFor69,
        errors: [ValidationException]
    }

    @input
    structure InputFor69 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor69 {
        @required
        output: String,
    }


    @http(uri: "/testhandler70/{region}", method: "POST")
    operation CustomOp70 {
        input: InputFor70,
        output: OutputFor70,
        errors: [ValidationException]
    }

    @input
    structure InputFor70 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor70 {
        @required
        output: String,
    }


    @http(uri: "/testhandler71/{region}", method: "POST")
    operation CustomOp71 {
        input: InputFor71,
        output: OutputFor71,
        errors: [ValidationException]
    }

    @input
    structure InputFor71 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor71 {
        @required
        output: String,
    }


    @http(uri: "/testhandler72/{region}", method: "POST")
    operation CustomOp72 {
        input: InputFor72,
        output: OutputFor72,
        errors: [ValidationException]
    }

    @input
    structure InputFor72 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor72 {
        @required
        output: String,
    }


    @http(uri: "/testhandler73/{region}", method: "POST")
    operation CustomOp73 {
        input: InputFor73,
        output: OutputFor73,
        errors: [ValidationException]
    }

    @input
    structure InputFor73 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor73 {
        @required
        output: String,
    }


    @http(uri: "/testhandler74/{region}", method: "POST")
    operation CustomOp74 {
        input: InputFor74,
        output: OutputFor74,
        errors: [ValidationException]
    }

    @input
    structure InputFor74 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor74 {
        @required
        output: String,
    }


    @http(uri: "/testhandler75/{region}", method: "POST")
    operation CustomOp75 {
        input: InputFor75,
        output: OutputFor75,
        errors: [ValidationException]
    }

    @input
    structure InputFor75 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor75 {
        @required
        output: String,
    }


    @http(uri: "/testhandler76/{region}", method: "POST")
    operation CustomOp76 {
        input: InputFor76,
        output: OutputFor76,
        errors: [ValidationException]
    }

    @input
    structure InputFor76 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor76 {
        @required
        output: String,
    }


    @http(uri: "/testhandler77/{region}", method: "POST")
    operation CustomOp77 {
        input: InputFor77,
        output: OutputFor77,
        errors: [ValidationException]
    }

    @input
    structure InputFor77 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor77 {
        @required
        output: String,
    }


    @http(uri: "/testhandler78/{region}", method: "POST")
    operation CustomOp78 {
        input: InputFor78,
        output: OutputFor78,
        errors: [ValidationException]
    }

    @input
    structure InputFor78 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor78 {
        @required
        output: String,
    }


    @http(uri: "/testhandler79/{region}", method: "POST")
    operation CustomOp79 {
        input: InputFor79,
        output: OutputFor79,
        errors: [ValidationException]
    }

    @input
    structure InputFor79 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor79 {
        @required
        output: String,
    }


    @http(uri: "/testhandler80/{region}", method: "POST")
    operation CustomOp80 {
        input: InputFor80,
        output: OutputFor80,
        errors: [ValidationException]
    }

    @input
    structure InputFor80 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor80 {
        @required
        output: String,
    }


    @http(uri: "/testhandler81/{region}", method: "POST")
    operation CustomOp81 {
        input: InputFor81,
        output: OutputFor81,
        errors: [ValidationException]
    }

    @input
    structure InputFor81 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor81 {
        @required
        output: String,
    }


    @http(uri: "/testhandler82/{region}", method: "POST")
    operation CustomOp82 {
        input: InputFor82,
        output: OutputFor82,
        errors: [ValidationException]
    }

    @input
    structure InputFor82 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor82 {
        @required
        output: String,
    }


    @http(uri: "/testhandler83/{region}", method: "POST")
    operation CustomOp83 {
        input: InputFor83,
        output: OutputFor83,
        errors: [ValidationException]
    }

    @input
    structure InputFor83 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor83 {
        @required
        output: String,
    }


    @http(uri: "/testhandler84/{region}", method: "POST")
    operation CustomOp84 {
        input: InputFor84,
        output: OutputFor84,
        errors: [ValidationException]
    }

    @input
    structure InputFor84 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor84 {
        @required
        output: String,
    }


    @http(uri: "/testhandler85/{region}", method: "POST")
    operation CustomOp85 {
        input: InputFor85,
        output: OutputFor85,
        errors: [ValidationException]
    }

    @input
    structure InputFor85 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor85 {
        @required
        output: String,
    }


    @http(uri: "/testhandler86/{region}", method: "POST")
    operation CustomOp86 {
        input: InputFor86,
        output: OutputFor86,
        errors: [ValidationException]
    }

    @input
    structure InputFor86 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor86 {
        @required
        output: String,
    }


    @http(uri: "/testhandler87/{region}", method: "POST")
    operation CustomOp87 {
        input: InputFor87,
        output: OutputFor87,
        errors: [ValidationException]
    }

    @input
    structure InputFor87 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor87 {
        @required
        output: String,
    }


    @http(uri: "/testhandler88/{region}", method: "POST")
    operation CustomOp88 {
        input: InputFor88,
        output: OutputFor88,
        errors: [ValidationException]
    }

    @input
    structure InputFor88 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor88 {
        @required
        output: String,
    }


    @http(uri: "/testhandler89/{region}", method: "POST")
    operation CustomOp89 {
        input: InputFor89,
        output: OutputFor89,
        errors: [ValidationException]
    }

    @input
    structure InputFor89 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor89 {
        @required
        output: String,
    }


    @http(uri: "/testhandler90/{region}", method: "POST")
    operation CustomOp90 {
        input: InputFor90,
        output: OutputFor90,
        errors: [ValidationException]
    }

    @input
    structure InputFor90 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor90 {
        @required
        output: String,
    }


    @http(uri: "/testhandler91/{region}", method: "POST")
    operation CustomOp91 {
        input: InputFor91,
        output: OutputFor91,
        errors: [ValidationException]
    }

    @input
    structure InputFor91 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor91 {
        @required
        output: String,
    }


    @http(uri: "/testhandler92/{region}", method: "POST")
    operation CustomOp92 {
        input: InputFor92,
        output: OutputFor92,
        errors: [ValidationException]
    }

    @input
    structure InputFor92 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor92 {
        @required
        output: String,
    }


    @http(uri: "/testhandler93/{region}", method: "POST")
    operation CustomOp93 {
        input: InputFor93,
        output: OutputFor93,
        errors: [ValidationException]
    }

    @input
    structure InputFor93 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor93 {
        @required
        output: String,
    }


    @http(uri: "/testhandler94/{region}", method: "POST")
    operation CustomOp94 {
        input: InputFor94,
        output: OutputFor94,
        errors: [ValidationException]
    }

    @input
    structure InputFor94 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor94 {
        @required
        output: String,
    }


    @http(uri: "/testhandler95/{region}", method: "POST")
    operation CustomOp95 {
        input: InputFor95,
        output: OutputFor95,
        errors: [ValidationException]
    }

    @input
    structure InputFor95 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor95 {
        @required
        output: String,
    }


    @http(uri: "/testhandler96/{region}", method: "POST")
    operation CustomOp96 {
        input: InputFor96,
        output: OutputFor96,
        errors: [ValidationException]
    }

    @input
    structure InputFor96 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor96 {
        @required
        output: String,
    }


    @http(uri: "/testhandler97/{region}", method: "POST")
    operation CustomOp97 {
        input: InputFor97,
        output: OutputFor97,
        errors: [ValidationException]
    }

    @input
    structure InputFor97 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor97 {
        @required
        output: String,
    }


    @http(uri: "/testhandler98/{region}", method: "POST")
    operation CustomOp98 {
        input: InputFor98,
        output: OutputFor98,
        errors: [ValidationException]
    }

    @input
    structure InputFor98 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor98 {
        @required
        output: String,
    }


    @http(uri: "/testhandler99/{region}", method: "POST")
    operation CustomOp99 {
        input: InputFor99,
        output: OutputFor99,
        errors: [ValidationException]
    }

    @input
    structure InputFor99 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor99 {
        @required
        output: String,
    }


    @http(uri: "/testhandler100/{region}", method: "POST")
    operation CustomOp100 {
        input: InputFor100,
        output: OutputFor100,
        errors: [ValidationException]
    }

    @input
    structure InputFor100 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor100 {
        @required
        output: String,
    }


    @http(uri: "/testhandler101/{region}", method: "POST")
    operation CustomOp101 {
        input: InputFor101,
        output: OutputFor101,
        errors: [ValidationException]
    }

    @input
    structure InputFor101 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor101 {
        @required
        output: String,
    }


    @http(uri: "/testhandler102/{region}", method: "POST")
    operation CustomOp102 {
        input: InputFor102,
        output: OutputFor102,
        errors: [ValidationException]
    }

    @input
    structure InputFor102 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor102 {
        @required
        output: String,
    }


    @http(uri: "/testhandler103/{region}", method: "POST")
    operation CustomOp103 {
        input: InputFor103,
        output: OutputFor103,
        errors: [ValidationException]
    }

    @input
    structure InputFor103 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor103 {
        @required
        output: String,
    }


    @http(uri: "/testhandler104/{region}", method: "POST")
    operation CustomOp104 {
        input: InputFor104,
        output: OutputFor104,
        errors: [ValidationException]
    }

    @input
    structure InputFor104 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor104 {
        @required
        output: String,
    }


    @http(uri: "/testhandler105/{region}", method: "POST")
    operation CustomOp105 {
        input: InputFor105,
        output: OutputFor105,
        errors: [ValidationException]
    }

    @input
    structure InputFor105 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor105 {
        @required
        output: String,
    }


    @http(uri: "/testhandler106/{region}", method: "POST")
    operation CustomOp106 {
        input: InputFor106,
        output: OutputFor106,
        errors: [ValidationException]
    }

    @input
    structure InputFor106 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor106 {
        @required
        output: String,
    }


    @http(uri: "/testhandler107/{region}", method: "POST")
    operation CustomOp107 {
        input: InputFor107,
        output: OutputFor107,
        errors: [ValidationException]
    }

    @input
    structure InputFor107 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor107 {
        @required
        output: String,
    }


    @http(uri: "/testhandler108/{region}", method: "POST")
    operation CustomOp108 {
        input: InputFor108,
        output: OutputFor108,
        errors: [ValidationException]
    }

    @input
    structure InputFor108 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor108 {
        @required
        output: String,
    }


    @http(uri: "/testhandler109/{region}", method: "POST")
    operation CustomOp109 {
        input: InputFor109,
        output: OutputFor109,
        errors: [ValidationException]
    }

    @input
    structure InputFor109 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor109 {
        @required
        output: String,
    }


    @http(uri: "/testhandler110/{region}", method: "POST")
    operation CustomOp110 {
        input: InputFor110,
        output: OutputFor110,
        errors: [ValidationException]
    }

    @input
    structure InputFor110 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor110 {
        @required
        output: String,
    }


    @http(uri: "/testhandler111/{region}", method: "POST")
    operation CustomOp111 {
        input: InputFor111,
        output: OutputFor111,
        errors: [ValidationException]
    }

    @input
    structure InputFor111 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor111 {
        @required
        output: String,
    }


    @http(uri: "/testhandler112/{region}", method: "POST")
    operation CustomOp112 {
        input: InputFor112,
        output: OutputFor112,
        errors: [ValidationException]
    }

    @input
    structure InputFor112 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor112 {
        @required
        output: String,
    }


    @http(uri: "/testhandler113/{region}", method: "POST")
    operation CustomOp113 {
        input: InputFor113,
        output: OutputFor113,
        errors: [ValidationException]
    }

    @input
    structure InputFor113 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor113 {
        @required
        output: String,
    }


    @http(uri: "/testhandler114/{region}", method: "POST")
    operation CustomOp114 {
        input: InputFor114,
        output: OutputFor114,
        errors: [ValidationException]
    }

    @input
    structure InputFor114 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor114 {
        @required
        output: String,
    }


    @http(uri: "/testhandler115/{region}", method: "POST")
    operation CustomOp115 {
        input: InputFor115,
        output: OutputFor115,
        errors: [ValidationException]
    }

    @input
    structure InputFor115 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor115 {
        @required
        output: String,
    }


    @http(uri: "/testhandler116/{region}", method: "POST")
    operation CustomOp116 {
        input: InputFor116,
        output: OutputFor116,
        errors: [ValidationException]
    }

    @input
    structure InputFor116 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor116 {
        @required
        output: String,
    }


    @http(uri: "/testhandler117/{region}", method: "POST")
    operation CustomOp117 {
        input: InputFor117,
        output: OutputFor117,
        errors: [ValidationException]
    }

    @input
    structure InputFor117 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor117 {
        @required
        output: String,
    }


    @http(uri: "/testhandler118/{region}", method: "POST")
    operation CustomOp118 {
        input: InputFor118,
        output: OutputFor118,
        errors: [ValidationException]
    }

    @input
    structure InputFor118 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor118 {
        @required
        output: String,
    }


    @http(uri: "/testhandler119/{region}", method: "POST")
    operation CustomOp119 {
        input: InputFor119,
        output: OutputFor119,
        errors: [ValidationException]
    }

    @input
    structure InputFor119 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor119 {
        @required
        output: String,
    }


    @http(uri: "/testhandler120/{region}", method: "POST")
    operation CustomOp120 {
        input: InputFor120,
        output: OutputFor120,
        errors: [ValidationException]
    }

    @input
    structure InputFor120 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor120 {
        @required
        output: String,
    }


    @http(uri: "/testhandler121/{region}", method: "POST")
    operation CustomOp121 {
        input: InputFor121,
        output: OutputFor121,
        errors: [ValidationException]
    }

    @input
    structure InputFor121 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor121 {
        @required
        output: String,
    }


    @http(uri: "/testhandler122/{region}", method: "POST")
    operation CustomOp122 {
        input: InputFor122,
        output: OutputFor122,
        errors: [ValidationException]
    }

    @input
    structure InputFor122 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor122 {
        @required
        output: String,
    }


    @http(uri: "/testhandler123/{region}", method: "POST")
    operation CustomOp123 {
        input: InputFor123,
        output: OutputFor123,
        errors: [ValidationException]
    }

    @input
    structure InputFor123 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor123 {
        @required
        output: String,
    }


    @http(uri: "/testhandler124/{region}", method: "POST")
    operation CustomOp124 {
        input: InputFor124,
        output: OutputFor124,
        errors: [ValidationException]
    }

    @input
    structure InputFor124 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor124 {
        @required
        output: String,
    }


    @http(uri: "/testhandler125/{region}", method: "POST")
    operation CustomOp125 {
        input: InputFor125,
        output: OutputFor125,
        errors: [ValidationException]
    }

    @input
    structure InputFor125 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor125 {
        @required
        output: String,
    }


    @http(uri: "/testhandler126/{region}", method: "POST")
    operation CustomOp126 {
        input: InputFor126,
        output: OutputFor126,
        errors: [ValidationException]
    }

    @input
    structure InputFor126 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor126 {
        @required
        output: String,
    }


    @http(uri: "/testhandler127/{region}", method: "POST")
    operation CustomOp127 {
        input: InputFor127,
        output: OutputFor127,
        errors: [ValidationException]
    }

    @input
    structure InputFor127 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor127 {
        @required
        output: String,
    }


    @http(uri: "/testhandler128/{region}", method: "POST")
    operation CustomOp128 {
        input: InputFor128,
        output: OutputFor128,
        errors: [ValidationException]
    }

    @input
    structure InputFor128 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor128 {
        @required
        output: String,
    }


    @http(uri: "/testhandler129/{region}", method: "POST")
    operation CustomOp129 {
        input: InputFor129,
        output: OutputFor129,
        errors: [ValidationException]
    }

    @input
    structure InputFor129 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor129 {
        @required
        output: String,
    }


    @http(uri: "/testhandler130/{region}", method: "POST")
    operation CustomOp130 {
        input: InputFor130,
        output: OutputFor130,
        errors: [ValidationException]
    }

    @input
    structure InputFor130 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor130 {
        @required
        output: String,
    }


    @http(uri: "/testhandler131/{region}", method: "POST")
    operation CustomOp131 {
        input: InputFor131,
        output: OutputFor131,
        errors: [ValidationException]
    }

    @input
    structure InputFor131 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor131 {
        @required
        output: String,
    }


    @http(uri: "/testhandler132/{region}", method: "POST")
    operation CustomOp132 {
        input: InputFor132,
        output: OutputFor132,
        errors: [ValidationException]
    }

    @input
    structure InputFor132 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor132 {
        @required
        output: String,
    }


    @http(uri: "/testhandler133/{region}", method: "POST")
    operation CustomOp133 {
        input: InputFor133,
        output: OutputFor133,
        errors: [ValidationException]
    }

    @input
    structure InputFor133 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor133 {
        @required
        output: String,
    }


    @http(uri: "/testhandler134/{region}", method: "POST")
    operation CustomOp134 {
        input: InputFor134,
        output: OutputFor134,
        errors: [ValidationException]
    }

    @input
    structure InputFor134 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor134 {
        @required
        output: String,
    }


    @http(uri: "/testhandler135/{region}", method: "POST")
    operation CustomOp135 {
        input: InputFor135,
        output: OutputFor135,
        errors: [ValidationException]
    }

    @input
    structure InputFor135 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor135 {
        @required
        output: String,
    }


    @http(uri: "/testhandler136/{region}", method: "POST")
    operation CustomOp136 {
        input: InputFor136,
        output: OutputFor136,
        errors: [ValidationException]
    }

    @input
    structure InputFor136 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor136 {
        @required
        output: String,
    }


    @http(uri: "/testhandler137/{region}", method: "POST")
    operation CustomOp137 {
        input: InputFor137,
        output: OutputFor137,
        errors: [ValidationException]
    }

    @input
    structure InputFor137 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor137 {
        @required
        output: String,
    }


    @http(uri: "/testhandler138/{region}", method: "POST")
    operation CustomOp138 {
        input: InputFor138,
        output: OutputFor138,
        errors: [ValidationException]
    }

    @input
    structure InputFor138 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor138 {
        @required
        output: String,
    }


    @http(uri: "/testhandler139/{region}", method: "POST")
    operation CustomOp139 {
        input: InputFor139,
        output: OutputFor139,
        errors: [ValidationException]
    }

    @input
    structure InputFor139 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor139 {
        @required
        output: String,
    }


    @http(uri: "/testhandler140/{region}", method: "POST")
    operation CustomOp140 {
        input: InputFor140,
        output: OutputFor140,
        errors: [ValidationException]
    }

    @input
    structure InputFor140 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor140 {
        @required
        output: String,
    }


    @http(uri: "/testhandler141/{region}", method: "POST")
    operation CustomOp141 {
        input: InputFor141,
        output: OutputFor141,
        errors: [ValidationException]
    }

    @input
    structure InputFor141 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor141 {
        @required
        output: String,
    }


    @http(uri: "/testhandler142/{region}", method: "POST")
    operation CustomOp142 {
        input: InputFor142,
        output: OutputFor142,
        errors: [ValidationException]
    }

    @input
    structure InputFor142 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor142 {
        @required
        output: String,
    }


    @http(uri: "/testhandler143/{region}", method: "POST")
    operation CustomOp143 {
        input: InputFor143,
        output: OutputFor143,
        errors: [ValidationException]
    }

    @input
    structure InputFor143 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor143 {
        @required
        output: String,
    }


    @http(uri: "/testhandler144/{region}", method: "POST")
    operation CustomOp144 {
        input: InputFor144,
        output: OutputFor144,
        errors: [ValidationException]
    }

    @input
    structure InputFor144 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor144 {
        @required
        output: String,
    }


    @http(uri: "/testhandler145/{region}", method: "POST")
    operation CustomOp145 {
        input: InputFor145,
        output: OutputFor145,
        errors: [ValidationException]
    }

    @input
    structure InputFor145 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor145 {
        @required
        output: String,
    }


    @http(uri: "/testhandler146/{region}", method: "POST")
    operation CustomOp146 {
        input: InputFor146,
        output: OutputFor146,
        errors: [ValidationException]
    }

    @input
    structure InputFor146 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor146 {
        @required
        output: String,
    }


    @http(uri: "/testhandler147/{region}", method: "POST")
    operation CustomOp147 {
        input: InputFor147,
        output: OutputFor147,
        errors: [ValidationException]
    }

    @input
    structure InputFor147 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor147 {
        @required
        output: String,
    }


    @http(uri: "/testhandler148/{region}", method: "POST")
    operation CustomOp148 {
        input: InputFor148,
        output: OutputFor148,
        errors: [ValidationException]
    }

    @input
    structure InputFor148 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor148 {
        @required
        output: String,
    }


    @http(uri: "/testhandler149/{region}", method: "POST")
    operation CustomOp149 {
        input: InputFor149,
        output: OutputFor149,
        errors: [ValidationException]
    }

    @input
    structure InputFor149 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor149 {
        @required
        output: String,
    }


    @http(uri: "/testhandler150/{region}", method: "POST")
    operation CustomOp150 {
        input: InputFor150,
        output: OutputFor150,
        errors: [ValidationException]
    }

    @input
    structure InputFor150 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor150 {
        @required
        output: String,
    }


    @http(uri: "/testhandler151/{region}", method: "POST")
    operation CustomOp151 {
        input: InputFor151,
        output: OutputFor151,
        errors: [ValidationException]
    }

    @input
    structure InputFor151 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor151 {
        @required
        output: String,
    }


    @http(uri: "/testhandler152/{region}", method: "POST")
    operation CustomOp152 {
        input: InputFor152,
        output: OutputFor152,
        errors: [ValidationException]
    }

    @input
    structure InputFor152 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor152 {
        @required
        output: String,
    }


    @http(uri: "/testhandler153/{region}", method: "POST")
    operation CustomOp153 {
        input: InputFor153,
        output: OutputFor153,
        errors: [ValidationException]
    }

    @input
    structure InputFor153 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor153 {
        @required
        output: String,
    }


    @http(uri: "/testhandler154/{region}", method: "POST")
    operation CustomOp154 {
        input: InputFor154,
        output: OutputFor154,
        errors: [ValidationException]
    }

    @input
    structure InputFor154 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor154 {
        @required
        output: String,
    }


    @http(uri: "/testhandler155/{region}", method: "POST")
    operation CustomOp155 {
        input: InputFor155,
        output: OutputFor155,
        errors: [ValidationException]
    }

    @input
    structure InputFor155 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor155 {
        @required
        output: String,
    }


    @http(uri: "/testhandler156/{region}", method: "POST")
    operation CustomOp156 {
        input: InputFor156,
        output: OutputFor156,
        errors: [ValidationException]
    }

    @input
    structure InputFor156 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor156 {
        @required
        output: String,
    }


    @http(uri: "/testhandler157/{region}", method: "POST")
    operation CustomOp157 {
        input: InputFor157,
        output: OutputFor157,
        errors: [ValidationException]
    }

    @input
    structure InputFor157 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor157 {
        @required
        output: String,
    }


    @http(uri: "/testhandler158/{region}", method: "POST")
    operation CustomOp158 {
        input: InputFor158,
        output: OutputFor158,
        errors: [ValidationException]
    }

    @input
    structure InputFor158 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor158 {
        @required
        output: String,
    }


    @http(uri: "/testhandler159/{region}", method: "POST")
    operation CustomOp159 {
        input: InputFor159,
        output: OutputFor159,
        errors: [ValidationException]
    }

    @input
    structure InputFor159 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor159 {
        @required
        output: String,
    }


    @http(uri: "/testhandler160/{region}", method: "POST")
    operation CustomOp160 {
        input: InputFor160,
        output: OutputFor160,
        errors: [ValidationException]
    }

    @input
    structure InputFor160 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor160 {
        @required
        output: String,
    }


    @http(uri: "/testhandler161/{region}", method: "POST")
    operation CustomOp161 {
        input: InputFor161,
        output: OutputFor161,
        errors: [ValidationException]
    }

    @input
    structure InputFor161 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor161 {
        @required
        output: String,
    }


    @http(uri: "/testhandler162/{region}", method: "POST")
    operation CustomOp162 {
        input: InputFor162,
        output: OutputFor162,
        errors: [ValidationException]
    }

    @input
    structure InputFor162 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor162 {
        @required
        output: String,
    }


    @http(uri: "/testhandler163/{region}", method: "POST")
    operation CustomOp163 {
        input: InputFor163,
        output: OutputFor163,
        errors: [ValidationException]
    }

    @input
    structure InputFor163 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor163 {
        @required
        output: String,
    }


    @http(uri: "/testhandler164/{region}", method: "POST")
    operation CustomOp164 {
        input: InputFor164,
        output: OutputFor164,
        errors: [ValidationException]
    }

    @input
    structure InputFor164 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor164 {
        @required
        output: String,
    }


    @http(uri: "/testhandler165/{region}", method: "POST")
    operation CustomOp165 {
        input: InputFor165,
        output: OutputFor165,
        errors: [ValidationException]
    }

    @input
    structure InputFor165 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor165 {
        @required
        output: String,
    }


    @http(uri: "/testhandler166/{region}", method: "POST")
    operation CustomOp166 {
        input: InputFor166,
        output: OutputFor166,
        errors: [ValidationException]
    }

    @input
    structure InputFor166 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor166 {
        @required
        output: String,
    }


    @http(uri: "/testhandler167/{region}", method: "POST")
    operation CustomOp167 {
        input: InputFor167,
        output: OutputFor167,
        errors: [ValidationException]
    }

    @input
    structure InputFor167 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor167 {
        @required
        output: String,
    }


    @http(uri: "/testhandler168/{region}", method: "POST")
    operation CustomOp168 {
        input: InputFor168,
        output: OutputFor168,
        errors: [ValidationException]
    }

    @input
    structure InputFor168 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor168 {
        @required
        output: String,
    }


    @http(uri: "/testhandler169/{region}", method: "POST")
    operation CustomOp169 {
        input: InputFor169,
        output: OutputFor169,
        errors: [ValidationException]
    }

    @input
    structure InputFor169 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor169 {
        @required
        output: String,
    }


    @http(uri: "/testhandler170/{region}", method: "POST")
    operation CustomOp170 {
        input: InputFor170,
        output: OutputFor170,
        errors: [ValidationException]
    }

    @input
    structure InputFor170 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor170 {
        @required
        output: String,
    }


    @http(uri: "/testhandler171/{region}", method: "POST")
    operation CustomOp171 {
        input: InputFor171,
        output: OutputFor171,
        errors: [ValidationException]
    }

    @input
    structure InputFor171 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor171 {
        @required
        output: String,
    }


    @http(uri: "/testhandler172/{region}", method: "POST")
    operation CustomOp172 {
        input: InputFor172,
        output: OutputFor172,
        errors: [ValidationException]
    }

    @input
    structure InputFor172 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor172 {
        @required
        output: String,
    }


    @http(uri: "/testhandler173/{region}", method: "POST")
    operation CustomOp173 {
        input: InputFor173,
        output: OutputFor173,
        errors: [ValidationException]
    }

    @input
    structure InputFor173 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor173 {
        @required
        output: String,
    }


    @http(uri: "/testhandler174/{region}", method: "POST")
    operation CustomOp174 {
        input: InputFor174,
        output: OutputFor174,
        errors: [ValidationException]
    }

    @input
    structure InputFor174 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor174 {
        @required
        output: String,
    }


    @http(uri: "/testhandler175/{region}", method: "POST")
    operation CustomOp175 {
        input: InputFor175,
        output: OutputFor175,
        errors: [ValidationException]
    }

    @input
    structure InputFor175 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor175 {
        @required
        output: String,
    }


    @http(uri: "/testhandler176/{region}", method: "POST")
    operation CustomOp176 {
        input: InputFor176,
        output: OutputFor176,
        errors: [ValidationException]
    }

    @input
    structure InputFor176 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor176 {
        @required
        output: String,
    }


    @http(uri: "/testhandler177/{region}", method: "POST")
    operation CustomOp177 {
        input: InputFor177,
        output: OutputFor177,
        errors: [ValidationException]
    }

    @input
    structure InputFor177 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor177 {
        @required
        output: String,
    }


    @http(uri: "/testhandler178/{region}", method: "POST")
    operation CustomOp178 {
        input: InputFor178,
        output: OutputFor178,
        errors: [ValidationException]
    }

    @input
    structure InputFor178 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor178 {
        @required
        output: String,
    }


    @http(uri: "/testhandler179/{region}", method: "POST")
    operation CustomOp179 {
        input: InputFor179,
        output: OutputFor179,
        errors: [ValidationException]
    }

    @input
    structure InputFor179 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor179 {
        @required
        output: String,
    }


    @http(uri: "/testhandler180/{region}", method: "POST")
    operation CustomOp180 {
        input: InputFor180,
        output: OutputFor180,
        errors: [ValidationException]
    }

    @input
    structure InputFor180 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor180 {
        @required
        output: String,
    }


    @http(uri: "/testhandler181/{region}", method: "POST")
    operation CustomOp181 {
        input: InputFor181,
        output: OutputFor181,
        errors: [ValidationException]
    }

    @input
    structure InputFor181 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor181 {
        @required
        output: String,
    }


    @http(uri: "/testhandler182/{region}", method: "POST")
    operation CustomOp182 {
        input: InputFor182,
        output: OutputFor182,
        errors: [ValidationException]
    }

    @input
    structure InputFor182 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor182 {
        @required
        output: String,
    }


    @http(uri: "/testhandler183/{region}", method: "POST")
    operation CustomOp183 {
        input: InputFor183,
        output: OutputFor183,
        errors: [ValidationException]
    }

    @input
    structure InputFor183 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor183 {
        @required
        output: String,
    }


    @http(uri: "/testhandler184/{region}", method: "POST")
    operation CustomOp184 {
        input: InputFor184,
        output: OutputFor184,
        errors: [ValidationException]
    }

    @input
    structure InputFor184 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor184 {
        @required
        output: String,
    }


    @http(uri: "/testhandler185/{region}", method: "POST")
    operation CustomOp185 {
        input: InputFor185,
        output: OutputFor185,
        errors: [ValidationException]
    }

    @input
    structure InputFor185 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor185 {
        @required
        output: String,
    }


    @http(uri: "/testhandler186/{region}", method: "POST")
    operation CustomOp186 {
        input: InputFor186,
        output: OutputFor186,
        errors: [ValidationException]
    }

    @input
    structure InputFor186 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor186 {
        @required
        output: String,
    }


    @http(uri: "/testhandler187/{region}", method: "POST")
    operation CustomOp187 {
        input: InputFor187,
        output: OutputFor187,
        errors: [ValidationException]
    }

    @input
    structure InputFor187 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor187 {
        @required
        output: String,
    }


    @http(uri: "/testhandler188/{region}", method: "POST")
    operation CustomOp188 {
        input: InputFor188,
        output: OutputFor188,
        errors: [ValidationException]
    }

    @input
    structure InputFor188 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor188 {
        @required
        output: String,
    }


    @http(uri: "/testhandler189/{region}", method: "POST")
    operation CustomOp189 {
        input: InputFor189,
        output: OutputFor189,
        errors: [ValidationException]
    }

    @input
    structure InputFor189 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor189 {
        @required
        output: String,
    }


    @http(uri: "/testhandler190/{region}", method: "POST")
    operation CustomOp190 {
        input: InputFor190,
        output: OutputFor190,
        errors: [ValidationException]
    }

    @input
    structure InputFor190 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor190 {
        @required
        output: String,
    }


    @http(uri: "/testhandler191/{region}", method: "POST")
    operation CustomOp191 {
        input: InputFor191,
        output: OutputFor191,
        errors: [ValidationException]
    }

    @input
    structure InputFor191 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor191 {
        @required
        output: String,
    }


    @http(uri: "/testhandler192/{region}", method: "POST")
    operation CustomOp192 {
        input: InputFor192,
        output: OutputFor192,
        errors: [ValidationException]
    }

    @input
    structure InputFor192 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor192 {
        @required
        output: String,
    }


    @http(uri: "/testhandler193/{region}", method: "POST")
    operation CustomOp193 {
        input: InputFor193,
        output: OutputFor193,
        errors: [ValidationException]
    }

    @input
    structure InputFor193 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor193 {
        @required
        output: String,
    }


    @http(uri: "/testhandler194/{region}", method: "POST")
    operation CustomOp194 {
        input: InputFor194,
        output: OutputFor194,
        errors: [ValidationException]
    }

    @input
    structure InputFor194 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor194 {
        @required
        output: String,
    }


    @http(uri: "/testhandler195/{region}", method: "POST")
    operation CustomOp195 {
        input: InputFor195,
        output: OutputFor195,
        errors: [ValidationException]
    }

    @input
    structure InputFor195 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor195 {
        @required
        output: String,
    }


    @http(uri: "/testhandler196/{region}", method: "POST")
    operation CustomOp196 {
        input: InputFor196,
        output: OutputFor196,
        errors: [ValidationException]
    }

    @input
    structure InputFor196 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor196 {
        @required
        output: String,
    }


    @http(uri: "/testhandler197/{region}", method: "POST")
    operation CustomOp197 {
        input: InputFor197,
        output: OutputFor197,
        errors: [ValidationException]
    }

    @input
    structure InputFor197 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor197 {
        @required
        output: String,
    }


    @http(uri: "/testhandler198/{region}", method: "POST")
    operation CustomOp198 {
        input: InputFor198,
        output: OutputFor198,
        errors: [ValidationException]
    }

    @input
    structure InputFor198 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor198 {
        @required
        output: String,
    }


    @http(uri: "/testhandler199/{region}", method: "POST")
    operation CustomOp199 {
        input: InputFor199,
        output: OutputFor199,
        errors: [ValidationException]
    }

    @input
    structure InputFor199 {
        @httpLabel
        @required
        region: String,
    }

    @output
    structure OutputFor199 {
        @required
        output: String,
    }

