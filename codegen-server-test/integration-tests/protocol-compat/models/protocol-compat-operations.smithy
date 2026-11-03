$version: "2"

// Shared operations for generated-server compatibility tests: bodiless REST routes, response
// bindings, structured and streaming payloads, prefix headers, labels, and sensitive outputs.
// Generated serde and schema serde service variants use the same operations to compare wire results.

// One operation per PR #4894 review finding. Each is bound to every REST and RPC protocol so the
// same wire request can be observed on a sole-protocol service and on multi-protocol services.
namespace com.example.review

// Finding: bodiless REST requests (label only) must not need `Content-Type`.
@readonly
@http(method: "GET", uri: "/widgets/{id}")
operation GetWidget {
    input := {
        @required
        @httpLabel
        id: String
    }
    output := {
        id: String
    }
}

@idempotent
@http(method: "DELETE", uri: "/widgets/{id}")
operation DeleteWidget {
    input := {
        @required
        @httpLabel
        id: String
    }
    output := {}
}

// No input at all.
@http(method: "POST", uri: "/ping")
operation Ping {
    output := {
        message: String
    }
}

// Finding: RPC protocols must ignore REST response bindings.
@http(method: "POST", uri: "/response-code", code: 201)
operation ResponseCode {
    output := {
        value: String
    }
}

@http(method: "POST", uri: "/response-code-member")
operation ResponseCodeMember {
    output := {
        @httpResponseCode
        status: Integer
        value: String
    }
}

@http(method: "POST", uri: "/response-header")
operation ResponseHeader {
    output := {
        @httpHeader("x-out")
        header: String
        value: String
    }
}

// Finding: a structure `@httpPayload` member next to a header member.
@http(method: "POST", uri: "/struct-payload")
operation StructPayload {
    input := {
        @httpHeader("x-h")
        h: String
        @httpPayload
        payload: Inner
    }
    output := {
        @httpHeader("x-h")
        h: String
        @httpPayload
        payload: Inner
    }
}

structure Inner {
    a: String
}

// Finding: a raw streaming blob is not an event stream.
@readonly
@http(method: "GET", uri: "/download")
operation DownloadBlob {
    output := {
        @required
        @httpPayload
        data: StreamingBlob
    }
}

@streaming
blob StreamingBlob

// Finding: prefix header values are trimmed.
@http(method: "POST", uri: "/prefix")
operation PrefixHeaders {
    input := {
        @httpPrefixHeaders("x-meta-")
        meta: StringMap
    }
    output := {
        echoed: StringMap
    }
}

// Finding: `@mediaType` map values in prefix headers are base64-decoded.
@http(method: "POST", uri: "/media")
operation MediaTypeMap {
    input := {
        @httpPrefixHeaders("x-json-")
        values: JsonMap
    }
    output := {
        echoed: StringMap
    }
}

map StringMap {
    key: String
    value: String
}

map JsonMap {
    key: String
    value: JsonString
}

@mediaType("application/json")
string JsonString

// Finding: a trailing slash in the URI pattern must not break label extraction.
@readonly
@http(method: "GET", uri: "/pets/{name}/")
operation TrailingSlashLabel {
    input := {
        @required
        @httpLabel
        name: String
    }
    output := {
        name: String
    }
}

@readonly
@http(method: "GET", uri: "/data/{key+}/")
operation GreedyLabel {
    input := {
        @required
        @httpLabel
        key: String
    }
    output := {
        key: String
    }
}

// Finding: sensitive values must not be logged when response serialization fails.
@http(method: "POST", uri: "/sensitive")
operation SensitiveAmount {
    output := {
        amount: SecretAmount
    }
}

@sensitive
bigDecimal SecretAmount

// A timestamp formatted as `date-time` that the formatter cannot represent, to force a serialization failure.
@http(method: "POST", uri: "/sensitive-timestamp")
operation SensitiveTimestamp {
    output := {
        when: SecretInstant
    }
}

@sensitive
@timestampFormat("date-time")
timestamp SecretInstant

// Finding: a `null` document payload is a value (`Some(Document::Null)`). JSON-only: restXml has no documents.
@http(method: "POST", uri: "/null-document")
operation NullDocument {
    input := {
        @httpPayload
        doc: Document
    }
    output := {
        kind: String
    }
}
