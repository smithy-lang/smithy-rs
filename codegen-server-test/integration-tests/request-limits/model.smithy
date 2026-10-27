$version: "2"

namespace integration.requestlimits

use aws.protocols#restJson1

/// Permanent fixture for request-body deadlines and buffered body size limits.
@restJson1
service RequestLimitsService {
    version: "2026-10-23"
    operations: [Echo, EchoWithOverride, Upload]
}

/// Echoes JSON using the default non-payload deadline.
@http(uri: "/echo", method: "POST")
operation Echo {
    input := {
        @required
        note: String
    }
    output := {
        @required
        note: String
    }
}

/// Echoes JSON using a shorter per-operation deadline on the same server.
@http(uri: "/echo-override", method: "POST")
operation EchoWithOverride {
    input := {
        @required
        note: String
    }
    output := {
        @required
        note: String
    }
}

/// Reads a non-streaming payload and reports its byte count.
@http(uri: "/upload", method: "POST")
operation Upload {
    input := {
        @httpPayload
        @required
        data: Blob
    }
    output := {
        @required
        size: Long
    }
}
