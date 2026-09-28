$version: "2"

namespace smithy.rust.codegen.compatibility

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#awsQuery
use aws.protocols#ec2Query
use aws.protocols#restJson1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor

@restJson1
service RestJsonService {
    version: "2024-01-01"
    operations: [RestOperation, RestEventStreamOperation, StreamingBlobOperation]
}

@restXml
service RestXmlService {
    version: "2024-01-01"
    operations: [RestOperation, StreamingBlobOperation]
}

@awsJson1_0
service AwsJson10Service {
    version: "2024-01-01"
    operations: [RpcOperation, RpcEventStreamOperation]
}

@awsJson1_1
service AwsJson11Service {
    version: "2024-01-01"
    operations: [RpcOperation, RpcEventStreamOperation]
}

@xmlNamespace(uri: "https://example.com/aws-query")
@awsQuery
service AwsQueryService {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@xmlNamespace(uri: "https://example.com/ec2-query")
@ec2Query
service Ec2QueryService {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@rpcv2Cbor
service RpcV2CborService {
    version: "2024-01-01"
    operations: [RpcOperation, RpcEventStreamOperation]
}

@http(uri: "/compatibility", method: "POST")
operation RestOperation {
    input := {
        value: String
    }
    output := {
        value: String
    }
    errors: [CompatibilityError]
}

operation RpcOperation {
    input := {
        value: String
    }
    output := {
        value: String
    }
    errors: [CompatibilityError]
}

// Event streams pull in aws-smithy-eventstream and the event stream
// serde/signing paths of the runtime crates.
@http(uri: "/event-stream", method: "POST")
operation RestEventStreamOperation {
    input := {
        @httpPayload
        events: CompatibilityEventStream
    }
    output := {
        @httpPayload
        events: CompatibilityEventStream
    }
    errors: [CompatibilityError]
}

operation RpcEventStreamOperation {
    input := {
        events: CompatibilityEventStream
    }
    output := {
        events: CompatibilityEventStream
    }
    errors: [CompatibilityError]
}

@streaming
union CompatibilityEventStream {
    message: CompatibilityEvent
}

structure CompatibilityEvent {
    @eventHeader
    name: String
    @eventPayload
    payload: Blob
}

// Streaming blobs pull in the ByteStream / streaming body paths of the
// runtime crates.
@http(uri: "/streaming-blob", method: "POST")
operation StreamingBlobOperation {
    input := {
        @httpPayload
        data: CompatibilityStreamingBlob = ""
    }
    output := {
        @httpPayload
        data: CompatibilityStreamingBlob = ""
    }
    errors: [CompatibilityError]
}

@streaming
blob CompatibilityStreamingBlob

@error("client")
structure CompatibilityError {
    message: String
}
