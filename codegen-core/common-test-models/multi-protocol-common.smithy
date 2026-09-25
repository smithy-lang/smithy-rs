$version: "2"

// Operations shared by the multi-protocol service and its single-protocol clients.
//
// `multi-protocol.smithy` binds these to a service declaring every built-in protocol, and
// `multi-protocol-<protocol>.smithy` binds them to the same service ID declaring one protocol each,
// so a client generated for each protocol can call the one server.
namespace com.example.multiprotocol

use smithy.framework#ValidationException

/// Exercises every HTTP binding kind on the REST protocols; RPC protocols carry all members in the body
/// and answer `200` whatever `@http` says.
@http(method: "POST", uri: "/greet/{name}", code: 201)
operation Greet {
    input := {
        @required
        @httpLabel
        @length(min: 1, max: 32)
        name: String

        @httpHeader("x-greeting")
        greeting: String

        @httpQuery("times")
        times: Integer

        tags: TagList
    }

    output := {
        @required
        message: String

        @httpHeader("x-greeted")
        greeted: String

        tags: TagList
    }

    errors: [
        ValidationException
        Unwelcome
    ]
}

list TagList {
    member: String
}

/// Raised for the name `intruder`.
@error("client")
@httpError(403)
structure Unwelcome {
    @required
    message: String

    // Not `name`: server codegen gives every error a `name()` method, which a `name` member collides with.
    who: String
}

/// Empty input and output.
@readonly
@http(method: "GET", uri: "/ping")
operation Ping {
    input := {}
    output := {}
}

/// A streaming blob upload. Only the REST protocols stream blobs.
@idempotent
@http(method: "PUT", uri: "/upload")
operation Upload {
    input := {
        @required
        @httpPayload
        data: Data
    }

    output := {
        @required
        size: Long
    }
}

@streaming
blob Data

/// The server streams `count` notes on `topic` back to the client.
@http(method: "POST", uri: "/subscribe")
operation Subscribe {
    input := {
        @required
        @httpHeader("x-topic")
        topic: String

        @required
        @httpHeader("x-count")
        count: Integer
    }

    output := {
        @required
        @httpPayload
        events: Events
    }
}

/// The client streams notes to the server, which counts them.
@http(method: "POST", uri: "/publish")
operation Publish {
    input := {
        @required
        @httpHeader("x-topic")
        topic: String

        @required
        @httpPayload
        events: Events
    }

    output := {
        @required
        received: Integer

        @required
        text: String
    }
}

@streaming
union Events {
    note: Note
}

// Event stream members cannot carry constraint traits, `@required` included, on the server.
structure Note {
    text: String
}
