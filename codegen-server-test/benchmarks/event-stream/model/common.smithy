$version: "2.0"
namespace benchmark.eventstream

@http(method: "POST", uri: "/upload", code: 200)
operation Upload { input: StreamInput, output: UploadOutput }

@http(method: "POST", uri: "/download", code: 200)
operation Download { input: DownloadInput, output: StreamOutput }

@http(method: "POST", uri: "/echo", code: 200)
operation Echo { input: StreamInput, output: StreamOutput }

structure StreamInput {
    @required
    @httpHeader("x-benchmark-kind")
    kind: String
    @required
    @httpHeader("x-benchmark-size")
    size: Integer
    @required
    @httpHeader("x-benchmark-count")
    count: Integer
    @httpPayload
    events: Events
}

structure DownloadInput {
    @required
    @httpHeader("x-benchmark-kind")
    kind: String
    @required
    @httpHeader("x-benchmark-size")
    size: Integer
    @required
    @httpHeader("x-benchmark-count")
    count: Integer
    @required
    @httpHeader("x-benchmark-error")
    fail: Boolean
}

structure StreamOutput {
    @required
    @httpHeader("x-benchmark-metadata")
    metadata: String
    @httpPayload
    events: Events
}

structure UploadOutput {
    @required
    count: Integer
    @required
    bytes: Long
}

@streaming
union Events {
    structured: StructuredEvent
    binary: BinaryEvent
    failure: StreamFailure
}

structure StructuredEvent {
    @eventHeader
    sequence: Long
    @eventPayload
    payload: StructuredData
}

structure StructuredData {
    text: String
    nested: Nested
    values: Values
}

structure Nested {
    label: String
    enabled: Boolean
}

list Values { member: Integer }

structure BinaryEvent {
    @eventHeader
    sequence: Long
    @eventPayload
    payload: Blob
}

@error("server")
structure StreamFailure {
    message: String
}
