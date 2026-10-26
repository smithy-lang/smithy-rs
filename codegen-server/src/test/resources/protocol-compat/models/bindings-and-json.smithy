$version: "2"

// Defines shared audit operations covering query/header bindings, URI labels, JSON bodies,
// payloads, response bindings, and modeled errors. Protocol-specific audit services import
// these shapes so corpus requests compare equivalent handler inputs and wire responses.

namespace compat.audit

// Source: ServerHttpBoundProtocolGenerator.serverRenderQueryStringParser and
// HttpHeaderValueParserGenerator.renderHeaderValueParser.
@http(method: "POST", uri: "/bindings")
operation Bindings {
    input := {
        @httpQuery("q") q: String
        @httpQuery("n") n: Integer
        @httpQuery("b") b: Boolean
        @httpQuery("list") list: Strings
        @httpQuery("dates") dates: EpochTimestamps
        @httpQueryParams params: StringLists
        @httpHeader("x-text") text: String
        @httpHeader("x-number") number: Integer
        @httpHeader("x-list") headerList: Strings
        @httpHeader("x-media") media: MediaString
        @httpHeader("x-date") date: Timestamp
        @httpHeader("x-dates") headerDates: Timestamps
        @httpPrefixHeaders("x-meta-") meta: StringMap
    }
    output := {}
}

@mediaType("application/json")
string MediaString
list Strings { member: String }
list Timestamps { member: Timestamp }
list EpochTimestamps { @timestampFormat("epoch-seconds") member: Timestamp }
map StringMap { key: String, value: String }
map StringLists { key: String, value: Strings }
map MediaMap { key: String, value: MediaString }

@http(method: "POST", uri: "/media-prefix")
operation MediaPrefix {
    input := { @httpPrefixHeaders("x-meta-") meta: MediaMap }
    output := {}
}

// Source: ServerHttpBoundProtocolGenerator.serverRenderUriPathParser.
@http(method: "GET", uri: "/label/{value}")
operation Label { input := { @required @httpLabel value: String }, output := {} }
@http(method: "GET", uri: "/greedy/{value+}/end")
operation Greedy { input := { @required @httpLabel value: String }, output := {} }
@http(method: "GET", uri: "/trailing/{value}/")
operation Trailing { input := { @required @httpLabel value: String }, output := {} }
@http(method: "GET", uri: "/greedy-trailing/{value+}/")
operation GreedyTrailing { input := { @required @httpLabel value: String }, output := {} }
@http(method: "GET", uri: "/number/{value}")
operation NumberLabel { input := { @required @httpLabel value: Integer }, output := {} }
@http(method: "GET", uri: "/timestamp/{value}")
operation TimestampLabel { input := { @required @httpLabel @timestampFormat("epoch-seconds") value: Timestamp }, output := {} }

// Source: JsonParserGenerator and ServerSchemaDeserializeGenerator.
@http(method: "POST", uri: "/body")
operation Body {
    input := {
        text: String
        integer: Integer
        long: Long
        float: Float
        double: Double
        boolean: Boolean
        timestamp: Timestamp
        blob: Blob
        list: Strings
        sparseList: SparseStrings
        map: StringMap
        sparseMap: SparseMap
        nested: Nested
        choice: Choice
        document: Document
        @default(17) defaulted: Integer
        @jsonName("renamed") original: String
        limited: Limited
        unique: UniqueStrings
        mode: Mode
        intMode: IntMode
    }
    output := {}
}
@sparse list SparseStrings { member: String }
@sparse map SparseMap { key: String, value: String }
structure Nested { text: String, @required required: String }
union Choice { text: String, integer: Integer }

// Source: HttpBindingGenerator.deserializePayloadBody.
@http(method: "POST", uri: "/string-payload")
operation StringPayload { input := { @httpPayload value: String }, output := {} }
@http(method: "POST", uri: "/blob-payload")
operation BlobPayload { input := { @httpPayload value: Blob }, output := {} }
@http(method: "POST", uri: "/struct-payload")
operation StructPayload { input := { @httpPayload value: Nested }, output := {} }
@http(method: "POST", uri: "/document-payload")
operation DocumentPayload { input := { @httpPayload value: Document }, output := {} }

// Source: HttpBindingGenerator.renderHeaders/renderPrefixHeader and
// ServerHttpBoundProtocolGenerator.serverRenderHttpResponseCode.
@http(method: "POST", uri: "/response")
operation Response {
    input := { scenario: String }
    output := {
        @httpHeader("x-text") text: String
        @httpHeader("x-secret") secret: Secret
        @httpHeader("x-list") list: Strings
        @httpHeader("x-number") number: Integer
        @httpHeader("x-bool") boolean: Boolean
        @httpHeader("x-date") date: Timestamp
        @httpPrefixHeaders("x-meta-") meta: StringMap
        @httpResponseCode code: Integer
        value: String
    }
}
@sensitive string Secret
map SecretMap { key: Secret, value: Secret }

// Source: ServerHttpSensitivityGenerator and StructureGenerator debug redaction.
@http(method: "POST", uri: "/sensitive/{value+}/end")
operation Sensitive {
    input := {
        @required @httpLabel value: Secret
        @httpQuery("secret") query: Secret
        @httpHeader("x-secret") header: Secret
        @httpPrefixHeaders("x-meta-") meta: SecretMap
        @httpQueryParams params: StringMap
        body: Secret
    }
    output := { @httpHeader("x-secret") secret: Secret }
}

@length(min: 2, max: 4) string Limited
@uniqueItems list UniqueStrings { member: String }
enum Mode {
    A = "a"
    B = "b"
}
intEnum IntMode {
    A = 1
    B = 2
}

// Source: HttpBindingGenerator.headerFmtFun and prefix-header serialization.
@http(method: "GET", uri: "/media-response")
operation MediaResponse { output := { @httpPrefixHeaders("x-meta-") meta: MediaMap } }

// Source: ServerHttpBoundProtocolGenerator.serverSerializeError/response headers.
@http(method: "POST", uri: "/modeled")
operation Modeled { input := { scenario: String }, output := {}, errors: [Oops] }
@error("client")
@httpError(409)
structure Oops {
    message: String
    @httpHeader("x-secret") secret: Secret
    @httpHeader("content-type") contentType: String
    @httpHeader("x-amzn-errortype") errorType: String
    @httpResponseCode code: Integer
}

// Source: EventStreamUnmarshallerGenerator and ServerStreamingOperationGenerator.
@http(method: "POST", uri: "/events")
operation Stream { input := { @required @httpPayload events: Events }, output := {} }
@streaming
union Events { Empty: EmptyEvent, Text: TextEvent, Headers: HeaderEvent, Raw: RawEvent }
structure EmptyEvent {}
structure TextEvent { text: String }
structure HeaderEvent { @eventHeader header: String, text: String }
structure RawEvent { @eventPayload value: String }
@streaming blob StreamingBytes
@http(method: "POST", uri: "/streaming-blob")
operation StreamingBlob { input := { @required @httpPayload value: StreamingBytes }, output := {} }
