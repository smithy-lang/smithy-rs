$version: "2"

namespace com.aws.example.esenum

use aws.protocols#restJson1

/// Event-stream service whose event payload contains an enum member but no
/// other constraint traits. Used to verify assumption A1's enum carve-out:
/// EnumTrait is excluded from the unsupported-constraints-in-event-streams
/// check, so this must generate successfully with the flag OFF.
@restJson1
service EventStreamEnumService {
    version: "1.0"
    operations: [
        CapturePokemonEnum
    ]
}

@http(uri: "/capture-enum/{region}", method: "POST")
operation CapturePokemonEnum {
    input := {
        @httpLabel
        @required
        region: String

        @httpPayload
        events: EnumAttemptEvents
    }
    output := {
        @httpPayload
        events: EnumCaptureEvents
    }
}

@streaming
union EnumAttemptEvents {
    event: EnumCapturingEvent
}

structure EnumCapturingEvent {
    @eventPayload
    payload: EnumCapturingPayload
}

structure EnumCapturingPayload {
    name: String
    pokeball: PokeballType
}

enum PokeballType {
    POKE_BALL = "poke"
    GREAT_BALL = "great"
    MASTER_BALL = "master"
}

@streaming
union EnumCaptureEvents {
    event: EnumPlainCaptureEvent
}

structure EnumPlainCaptureEvent {
    @eventPayload
    payload: Blob
}
