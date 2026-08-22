$version: "2"

namespace com.aws.example.esconstrained

use aws.protocols#restJson1
use smithy.framework#ValidationException

/// Pokémon-style event-stream service with a constrained member reachable
/// only through an event-stream payload. Used to verify assumption A1:
/// what codegen does with constraint traits inside event streams.
@restJson1
service EventStreamConstrainedService {
    version: "1.0"
    operations: [
        CapturePokemonConstrained
    ]
}

@http(uri: "/capture-constrained/{region}", method: "POST")
operation CapturePokemonConstrained {
    input := {
        @httpLabel
        @required
        region: String

        @httpPayload
        events: ConstrainedAttemptEvents
    }
    output := {
        @httpPayload
        events: ConstrainedCaptureEvents
    }
    errors: [
        ValidationException
    ]
}

@streaming
union ConstrainedAttemptEvents {
    event: ConstrainedCapturingEvent
}

structure ConstrainedCapturingEvent {
    @eventPayload
    payload: ConstrainedCapturingPayload
}

structure ConstrainedCapturingPayload {
    name: String

    @length(min: 1, max: 10)
    pokeball: String
}

@streaming
union ConstrainedCaptureEvents {
    event: PlainCaptureEvent
}

structure PlainCaptureEvent {
    @eventPayload
    payload: Blob
}
