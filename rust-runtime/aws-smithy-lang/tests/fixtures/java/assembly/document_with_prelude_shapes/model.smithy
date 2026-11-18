$version: "2"

namespace ex

/// A service.
@paginated(inputToken: "token", outputToken: "token", pageSize: "size")
service Things {
    version: "2024-01-01"
    operations: [ListThings, Ping]
}

@readonly
@paginated
operation ListThings {
    input := {
        token: String
        @range(min: 1, max: 100)
        size: Integer = 10
    }
    output := {
        token: String
        @required
        things: ThingList
    }
    errors: [Failure]
}

operation Ping {}

list ThingList {
    member: Thing
}

structure Thing {
    @required
    @length(min: 1)
    name: String

    @timestampFormat("date-time")
    created: Timestamp

    enabled: PrimitiveBoolean = false
}

@error("client")
structure Failure {
    message: String
}
