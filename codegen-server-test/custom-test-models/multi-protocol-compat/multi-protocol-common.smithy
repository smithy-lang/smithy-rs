$version: "2"

// Operations shared by the multi-protocol service and its single-protocol clients.
//
// `multi-protocol.smithy` binds these to a service declaring every built-in protocol, and
// `multi-protocol-<protocol>.smithy` binds them to the same service ID declaring one protocol each,
// so a client generated for each protocol can call the one server.
// Server projections also import one of the multi-protocol-awsjson-responses-*.smithy fixture files.
namespace smithy.rust.codegen.test.multiprotocol

use smithy.framework#ValidationException

/// Exercises every HTTP binding kind on REST. AWS JSON retains all members in the body,
/// with status and header handling selected by the responseBindings setting.
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

/// No modeled input or output: both default to smithy.api#Unit.
@readonly
@http(method: "GET", uri: "/no-input-output")
operation NoInputOutput {}

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

/// A structure payload whose XML member name deliberately differs from its target root.
@http(method: "POST", uri: "/payload/structure", code: 201)
operation StructurePayload {
    input := {
        @httpHeader("x-scenario")
        scenario: String

        @required
        @httpPayload
        @xmlName("MemberEnvelope")
        body: PayloadRecord
    }
    output := {
        @required
        @httpPayload
        @xmlName("MemberEnvelope")
        body: PayloadRecord
    }
    errors: [ValidationException]
}

/// An ordinary union payload also retains its field key under RPC protocols.
@http(method: "POST", uri: "/payload/union", code: 201)
operation UnionPayload {
    input := {
        @httpHeader("x-scenario")
        scenario: String

        @required
        @httpPayload
        @xmlName("MemberChoice")
        body: PayloadChoice
    }
    output := {
        @required
        @httpPayload
        @xmlName("MemberChoice")
        body: PayloadChoice
    }
}

/// REST response bindings remain ordinary document fields under the RPC protocols.
@idempotent
@http(method: "PUT", uri: "/things", code: 201)
operation ResponseBindings {
    input := {
        @httpHeader("x-scenario")
        scenario: String
    }
    output := {
        name: String
        @httpHeader("ETag")
        etag: String
        @httpPrefixHeaders("x-meta-")
        metadata: Metadata
        @httpResponseCode
        code: Integer
    }
}

/// Nested collections, union variants, blobs, timestamps, and populated modeled errors.
@http(method: "POST", uri: "/rich", code: 201)
operation RichData {
    input := {
        @httpHeader("x-scenario")
        scenario: String
        record: RichRecord
    }
    output := {
        @required
        record: RichRecord
    }
    errors: [ValidationException, RichFailure]
}

/// Missing output parameters exercise the generated builder's modeled defaults.
@http(method: "POST", uri: "/defaults", code: 201)
operation ModeledDefaults {
    input := {
        @httpHeader("x-scenario")
        scenario: String
    }
    output := {
        text: String = "builder"
        count: Integer = 17
        color: DefaultColor = "red"
    }
}

/// Sparse collection nulls are exercised independently from document values.
@http(method: "POST", uri: "/nullable", code: 201)
operation NullableValues {
    input := {
        @httpHeader("x-scenario")
        scenario: String
        values: SparseValues
        entries: SparseEntries
    }
    output := {
        values: SparseValues
        entries: SparseEntries
    }
}

/// Document values are available only on the JSON-only services.
@http(method: "POST", uri: "/document", code: 201)
operation DocumentValues {
    input := {
        @httpHeader("x-scenario")
        scenario: String
        document: Document
    }
    output := {
        document: Document
    }
}

@xmlName("PayloadTargetRoot")
structure PayloadRecord {
    @required
    label: Label
    child: NestedRecord
    items: NestedRecords
    lookup: NestedLookup
    choice: PayloadChoice
    bytes: Blob
    timestamp: Timestamp
}

@length(min: 1, max: 32)
string Label

structure NestedRecord {
    @required
    name: String
    count: Integer
    active: Boolean
}

list NestedRecords {
    member: NestedRecord
}

map NestedLookup {
    key: String
    value: NestedRecord
}

@xmlName("ChoiceTargetRoot")
union PayloadChoice {
    text: String
    record: NestedRecord
}

structure RichRecord {
    @required
    payload: PayloadRecord
    children: NestedRecords
    choices: Choices
    blobs: Blobs
}

list Choices {
    member: PayloadChoice
}

list Blobs {
    member: Blob
}

map Metadata {
    key: String
    value: String
}

enum DefaultColor {
    RED = "red"
    BLUE = "blue"
}

@sparse
list SparseValues {
    member: String
}

@sparse
map SparseEntries {
    key: String
    value: String
}

@error("client")
@httpError(422)
structure RichFailure {
    @required
    message: String
    details: NestedRecord
    @httpHeader("x-error-tag")
    etag: String
    @httpPrefixHeaders("x-error-meta-")
    metadata: Metadata
}

// BEGIN MULTIPROTOCOL NATIVE RESPONSE FIXTURES
apply smithy.rust.codegen.test.multiprotocol#StructurePayload @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonStructurePayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonStructurePayloadEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:empty"
        ],
        "body": "{\"label\":\"empty\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestxmlStructurePayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "<MemberEnvelope><label>fixture</label><child><name>child</name><count>7</count><active>true</active></child><items><member><name>first</name><count>1</count><active>false</active></member></items><lookup><entry><key>key</key><value><name>mapped</name><count>2</count></value></entry></lookup><choice><text>chosen</text></choice><bytes>YWJj</bytes><timestamp>1970-01-01T00:00:00Z</timestamp></MemberEnvelope>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRestxmlStructurePayloadEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:empty"
        ],
        "body": "<MemberEnvelope><label>empty</label></MemberEnvelope>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRpcv2CborStructurePayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "oWRib2R5p2VsYWJlbGdmaXh0dXJlZWNoaWxko2RuYW1lZWNoaWxkZWNvdW50B2ZhY3RpdmX1ZWl0ZW1zgaNkbmFtZWVmaXJzdGVjb3VudAFmYWN0aXZl9GZsb29rdXChY2tleaJkbmFtZWZtYXBwZWRlY291bnQCZmNob2ljZaFkdGV4dGZjaG9zZW5lYnl0ZXNDYWJjaXRpbWVzdGFtcMH7AAAAAAAAAAA=",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborStructurePayloadEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:empty"
        ],
        "body": "oWRib2R5oWVsYWJlbGVlbXB0eQ==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#UnionPayload @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonUnionPayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "{\"text\":\"chosen\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonUnionPayloadOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "{\"record\":{\"name\":\"union-record\",\"count\":3}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestxmlUnionPayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "<MemberChoice><text>chosen</text></MemberChoice>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRestxmlUnionPayloadOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "<MemberChoice><record><name>union-record</name><count>3</count></record></MemberChoice>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRpcv2CborUnionPayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "oWRib2R5oWR0ZXh0ZmNob3Nlbg==",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborUnionPayloadOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "oWRib2R5oWZyZWNvcmSiZG5hbWVsdW5pb24tcmVjb3JkZWNvdW50Aw==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ResponseBindings @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonResponseBindingsAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json",
            "etag": "\"abc\""
        },
        "params": {
            "name": "n",
            "etag": "\"abc\""
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{\"name\":\"n\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonResponseBindingsPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 299,
        "headers": {
            "content-type": "application/json",
            "etag": "\"abc\"",
            "x-meta-one": "1",
            "x-meta-two": "2"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\"",
            "metadata": {
                "one": "1",
                "two": "2"
            },
            "code": 299
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "{\"name\":\"n\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonResponseBindingsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 202,
        "headers": {
            "content-type": "application/json",
            "etag": "\"abc\"",
            "x-meta-one": "1",
            "x-meta-two": "2"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\"",
            "metadata": {
                "one": "1",
                "two": "2"
            },
            "code": 202
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "{\"name\":\"n\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestxmlResponseBindingsAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml",
            "etag": "\"abc\""
        },
        "params": {
            "name": "n",
            "etag": "\"abc\""
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "<ResponseBindingsOutput><name>n</name></ResponseBindingsOutput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRestxmlResponseBindingsPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 299,
        "headers": {
            "content-type": "application/xml",
            "etag": "\"abc\"",
            "x-meta-one": "1",
            "x-meta-two": "2"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\"",
            "metadata": {
                "one": "1",
                "two": "2"
            },
            "code": 299
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "<ResponseBindingsOutput><name>n</name></ResponseBindingsOutput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRestxmlResponseBindingsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 202,
        "headers": {
            "content-type": "application/xml",
            "etag": "\"abc\"",
            "x-meta-one": "1",
            "x-meta-two": "2"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\"",
            "metadata": {
                "one": "1",
                "two": "2"
            },
            "code": 202
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "<ResponseBindingsOutput><name>n</name></ResponseBindingsOutput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRpcv2CborResponseBindingsAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\""
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "forbidHeaders": [
            "etag",
            "x-meta-one",
            "x-meta-two"
        ],
        "body": "omRuYW1lYW5kZXRhZ2UiYWJjIg==",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborResponseBindingsPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\"",
            "metadata": {
                "one": "1",
                "two": "2"
            },
            "code": 299
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "forbidHeaders": [
            "etag",
            "x-meta-one",
            "x-meta-two"
        ],
        "body": "pGRuYW1lYW5kZXRhZ2UiYWJjImhtZXRhZGF0YaJjb25lYTFjdHdvYTJkY29kZRkBKw==",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborResponseBindingsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "name": "n",
            "etag": "\"abc\"",
            "metadata": {
                "one": "1",
                "two": "2"
            },
            "code": 202
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "forbidHeaders": [
            "etag",
            "x-meta-one",
            "x-meta-two"
        ],
        "body": "pGRuYW1lYW5kZXRhZ2UiYWJjImhtZXRhZGF0YaJjb25lYTFjdHdvYTJkY29kZRjK",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#RichData @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonRichDataPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "{\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestxmlRichDataPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "<RichDataOutput><record><payload><label>fixture</label><child><name>child</name><count>7</count><active>true</active></child><items><member><name>first</name><count>1</count><active>false</active></member></items><lookup><entry><key>key</key><value><name>mapped</name><count>2</count></value></entry></lookup><choice><text>chosen</text></choice><bytes>YWJj</bytes><timestamp>1970-01-01T00:00:00Z</timestamp></payload><children><member><name>extra</name><count>9</count></member></children><choices><member><text>list-choice</text></member><member><record><name>union-record</name><count>3</count></record></member></choices><blobs><member>YWJj</member><member>eHl6</member></blobs></record></RichDataOutput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRpcv2CborRichDataPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "oWZyZWNvcmSkZ3BheWxvYWSnZWxhYmVsZ2ZpeHR1cmVlY2hpbGSjZG5hbWVlY2hpbGRlY291bnQHZmFjdGl2ZfVlaXRlbXOBo2RuYW1lZWZpcnN0ZWNvdW50AWZhY3RpdmX0Zmxvb2t1cKFja2V5omRuYW1lZm1hcHBlZGVjb3VudAJmY2hvaWNloWR0ZXh0ZmNob3NlbmVieXRlc0NhYmNpdGltZXN0YW1wwfsAAAAAAAAAAGhjaGlsZHJlboGiZG5hbWVlZXh0cmFlY291bnQJZ2Nob2ljZXOCoWR0ZXh0a2xpc3QtY2hvaWNloWZyZWNvcmSiZG5hbWVsdW5pb24tcmVjb3JkZWNvdW50A2VibG9ic4JDYWJjQ3h5eg==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ModeledDefaults @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonModeledDefaultsDefault",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:default",
            "fuzz:populated"
        ],
        "body": "{\"text\":\"builder\",\"count\":17,\"color\":\"red\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonModeledDefaultsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "text": "explicit",
            "count": 23,
            "color": "blue"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "{\"text\":\"explicit\",\"count\":23,\"color\":\"blue\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestxmlModeledDefaultsDefault",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:default",
            "fuzz:populated"
        ],
        "body": "<ModeledDefaultsOutput><text>builder</text><count>17</count><color>red</color></ModeledDefaultsOutput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRestxmlModeledDefaultsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 201,
        "headers": {
            "content-type": "application/xml"
        },
        "params": {
            "text": "explicit",
            "count": 23,
            "color": "blue"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "<ModeledDefaultsOutput><text>explicit</text><count>23</count><color>blue</color></ModeledDefaultsOutput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRpcv2CborModeledDefaultsDefault",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:default",
            "fuzz:populated"
        ],
        "body": "o2R0ZXh0Z2J1aWxkZXJlY291bnQRZWNvbG9yY3JlZA==",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborModeledDefaultsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "text": "explicit",
            "count": 23,
            "color": "blue"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "o2R0ZXh0aGV4cGxpY2l0ZWNvdW50F2Vjb2xvcmRibHVl",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#NullableValues @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonNullableValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonNullableValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "values": [],
            "entries": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:empty"
        ],
        "body": "{\"values\":[],\"entries\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonNullableValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "values": [
                "first",
                null,
                "last"
            ],
            "entries": {
                "one": "value",
                "nil": null
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "{\"values\":[\"first\",null,\"last\"],\"entries\":{\"one\":\"value\",\"nil\":null}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRpcv2CborNullableValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "oA==",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborNullableValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "values": [],
            "entries": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:empty"
        ],
        "body": "omZ2YWx1ZXOAZ2VudHJpZXOg",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiRpcv2CborNullableValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 200,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "values": [
                "first",
                null,
                "last"
            ],
            "entries": {
                "one": "value",
                "nil": null
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "omZ2YWx1ZXODZWZpcnN09mRsYXN0Z2VudHJpZXOiY29uZWV2YWx1ZWNuaWz2",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#DocumentValues @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonDocumentValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonDocumentValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "document": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:empty"
        ],
        "body": "{\"document\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonDocumentValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "document": {
                "kind": "fixture",
                "nil": null,
                "count": 7
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:populated"
        ],
        "body": "{\"document\":{\"kind\":\"fixture\",\"nil\":null,\"count\":7}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestjsonDocumentValuesOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "headers": {
            "content-type": "application/json"
        },
        "params": {
            "document": null
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:override"
        ],
        "body": "{\"document\":null}",
        "bodyMediaType": "application/json"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#RichFailure @smithy.test#httpResponseTests(
[
    {
        "id": "MultiRestjsonRichFailureError",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restJson1",
        "code": 422,
        "headers": {
            "content-type": "application/json",
            "x-error-tag": "error-tag",
            "x-error-meta-reason": "fixture",
            "x-amzn-errortype": "RichFailure"
        },
        "params": {
            "message": "rejected",
            "details": {
                "name": "diagnostic",
                "count": 9,
                "active": false
            },
            "etag": "error-tag",
            "metadata": {
                "reason": "fixture"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:error"
        ],
        "body": "{\"message\":\"rejected\",\"details\":{\"name\":\"diagnostic\",\"count\":9,\"active\":false}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiRestxmlRichFailureError",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#restXml",
        "code": 422,
        "headers": {
            "content-type": "application/xml",
            "x-error-tag": "error-tag",
            "x-error-meta-reason": "fixture"
        },
        "params": {
            "message": "rejected",
            "details": {
                "name": "diagnostic",
                "count": 9,
                "active": false
            },
            "etag": "error-tag",
            "metadata": {
                "reason": "fixture"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:error"
        ],
        "body": "<Error><message>rejected</message><details><name>diagnostic</name><count>9</count><active>false</active></details></Error>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiRpcv2CborRichFailureError",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "code": 422,
        "headers": {
            "content-type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor"
        },
        "params": {
            "message": "rejected",
            "details": {
                "name": "diagnostic",
                "count": 9,
                "active": false
            },
            "etag": "error-tag",
            "metadata": {
                "reason": "fixture"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz:error"
        ],
        "forbidHeaders": [
            "x-error-tag",
            "x-error-meta-reason"
        ],
        "body": "pWdtZXNzYWdlaHJlamVjdGVkZ2RldGFpbHOjZG5hbWVqZGlhZ25vc3RpY2Vjb3VudAlmYWN0aXZl9GRldGFnaWVycm9yLXRhZ2htZXRhZGF0YaFmcmVhc29uZ2ZpeHR1cmVmX190eXBleDJzbWl0aHkucnVzdC5jb2RlZ2VuLnRlc3QubXVsdGlwcm90b2NvbCNSaWNoRmFpbHVyZQ==",
        "bodyMediaType": "application/cbor"
    }
]
)

// END MULTIPROTOCOL NATIVE RESPONSE FIXTURES

// BEGIN MULTIPROTOCOL SCENARIO REQUEST SEEDS
apply smithy.rust.codegen.test.multiprotocol#StructurePayload @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonStructurePayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/payload/structure",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonStructurePayloadEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/payload/structure",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "empty"
        },
        "params": {
            "scenario": "empty",
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"label\":\"empty\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestxmlStructurePayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/payload/structure",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "<MemberEnvelope><label>fixture</label><child><name>child</name><count>7</count><active>true</active></child><items><member><name>first</name><count>1</count><active>false</active></member></items><lookup><entry><key>key</key><value><name>mapped</name><count>2</count></value></entry></lookup><choice><text>chosen</text></choice><bytes>YWJj</bytes><timestamp>1970-01-01T00:00:00Z</timestamp></MemberEnvelope>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlStructurePayloadEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/payload/structure",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "empty"
        },
        "params": {
            "scenario": "empty",
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "<MemberEnvelope><label>empty</label></MemberEnvelope>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedAwsjson10StructurePayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.StructurePayload"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"body\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10StructurePayloadEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.StructurePayload"
        },
        "params": {
            "scenario": "empty",
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"scenario\":\"empty\",\"body\":{\"label\":\"empty\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11StructurePayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.StructurePayload"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"body\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11StructurePayloadEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.StructurePayload"
        },
        "params": {
            "scenario": "empty",
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"scenario\":\"empty\",\"body\":{\"label\":\"empty\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRpcv2CborStructurePayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/StructurePayload",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "label": "fixture",
                "child": {
                    "name": "child",
                    "count": 7,
                    "active": true
                },
                "items": [
                    {
                        "name": "first",
                        "count": 1,
                        "active": false
                    }
                ],
                "lookup": {
                    "key": {
                        "name": "mapped",
                        "count": 2
                    }
                },
                "choice": {
                    "text": "chosen"
                },
                "bytes": "abc",
                "timestamp": 0
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "omhzY2VuYXJpb2lwb3B1bGF0ZWRkYm9keadlbGFiZWxnZml4dHVyZWVjaGlsZKNkbmFtZWVjaGlsZGVjb3VudAdmYWN0aXZl9WVpdGVtc4GjZG5hbWVlZmlyc3RlY291bnQBZmFjdGl2ZfRmbG9va3VwoWNrZXmiZG5hbWVmbWFwcGVkZWNvdW50AmZjaG9pY2WhZHRleHRmY2hvc2VuZWJ5dGVzQ2FiY2l0aW1lc3RhbXDB+wAAAAAAAAAA",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborStructurePayloadEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/StructurePayload",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "empty",
            "body": {
                "label": "empty"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "omhzY2VuYXJpb2VlbXB0eWRib2R5oWVsYWJlbGVlbXB0eQ==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#UnionPayload @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonUnionPayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/payload/union",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"text\":\"chosen\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonUnionPayloadOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/payload/union",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override",
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"record\":{\"name\":\"union-record\",\"count\":3}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestxmlUnionPayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/payload/union",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "<MemberChoice><text>chosen</text></MemberChoice>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlUnionPayloadOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/payload/union",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override",
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "<MemberChoice><record><name>union-record</name><count>3</count></record></MemberChoice>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedAwsjson10UnionPayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.UnionPayload"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"body\":{\"text\":\"chosen\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10UnionPayloadOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.UnionPayload"
        },
        "params": {
            "scenario": "override",
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\",\"body\":{\"record\":{\"name\":\"union-record\",\"count\":3}}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11UnionPayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.UnionPayload"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"body\":{\"text\":\"chosen\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11UnionPayloadOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.UnionPayload"
        },
        "params": {
            "scenario": "override",
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\",\"body\":{\"record\":{\"name\":\"union-record\",\"count\":3}}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRpcv2CborUnionPayloadPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/UnionPayload",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "populated",
            "body": {
                "text": "chosen"
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "omhzY2VuYXJpb2lwb3B1bGF0ZWRkYm9keaFkdGV4dGZjaG9zZW4=",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborUnionPayloadOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/UnionPayload",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "override",
            "body": {
                "record": {
                    "name": "union-record",
                    "count": 3
                }
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "omhzY2VuYXJpb2hvdmVycmlkZWRib2R5oWZyZWNvcmSiZG5hbWVsdW5pb24tcmVjb3JkZWNvdW50Aw==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ResponseBindings @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonResponseBindingsAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "PUT",
        "uri": "/things",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "absent"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonResponseBindingsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "PUT",
        "uri": "/things",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonResponseBindingsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "PUT",
        "uri": "/things",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestxmlResponseBindingsAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "PUT",
        "uri": "/things",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "absent"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlResponseBindingsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "PUT",
        "uri": "/things",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlResponseBindingsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "PUT",
        "uri": "/things",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedAwsjson10ResponseBindingsAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.ResponseBindings"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "{\"scenario\":\"absent\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10ResponseBindingsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.ResponseBindings"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10ResponseBindingsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.ResponseBindings"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11ResponseBindingsAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.ResponseBindings"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "{\"scenario\":\"absent\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11ResponseBindingsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.ResponseBindings"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11ResponseBindingsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.ResponseBindings"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRpcv2CborResponseBindingsAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/ResponseBindings",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "oWhzY2VuYXJpb2ZhYnNlbnQ=",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborResponseBindingsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/ResponseBindings",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "oWhzY2VuYXJpb2lwb3B1bGF0ZWQ=",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborResponseBindingsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/ResponseBindings",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "oWhzY2VuYXJpb2hvdmVycmlkZQ==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#RichData @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonRichDataPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/rich",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonRichDataError",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/rich",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "error"
        },
        "params": {
            "scenario": "error",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:error"
        ],
        "body": "{\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestxmlRichDataPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/rich",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "<RichDataInput><record><payload><label>fixture</label><child><name>child</name><count>7</count><active>true</active></child><items><member><name>first</name><count>1</count><active>false</active></member></items><lookup><entry><key>key</key><value><name>mapped</name><count>2</count></value></entry></lookup><choice><text>chosen</text></choice><bytes>YWJj</bytes><timestamp>1970-01-01T00:00:00Z</timestamp></payload><children><member><name>extra</name><count>9</count></member></children><choices><member><text>list-choice</text></member><member><record><name>union-record</name><count>3</count></record></member></choices><blobs><member>YWJj</member><member>eHl6</member></blobs></record></RichDataInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlRichDataError",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/rich",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "error"
        },
        "params": {
            "scenario": "error",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:error"
        ],
        "body": "<RichDataInput><record><payload><label>fixture</label><child><name>child</name><count>7</count><active>true</active></child><items><member><name>first</name><count>1</count><active>false</active></member></items><lookup><entry><key>key</key><value><name>mapped</name><count>2</count></value></entry></lookup><choice><text>chosen</text></choice><bytes>YWJj</bytes><timestamp>1970-01-01T00:00:00Z</timestamp></payload><children><member><name>extra</name><count>9</count></member></children><choices><member><text>list-choice</text></member><member><record><name>union-record</name><count>3</count></record></member></choices><blobs><member>YWJj</member><member>eHl6</member></blobs></record></RichDataInput>",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedAwsjson10RichDataPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.RichData"
        },
        "params": {
            "scenario": "populated",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10RichDataError",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.RichData"
        },
        "params": {
            "scenario": "error",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:error"
        ],
        "body": "{\"scenario\":\"error\",\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11RichDataPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.RichData"
        },
        "params": {
            "scenario": "populated",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11RichDataError",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.RichData"
        },
        "params": {
            "scenario": "error",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:error"
        ],
        "body": "{\"scenario\":\"error\",\"record\":{\"payload\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0},\"children\":[{\"name\":\"extra\",\"count\":9}],\"choices\":[{\"text\":\"list-choice\"},{\"record\":{\"name\":\"union-record\",\"count\":3}}],\"blobs\":[\"YWJj\",\"eHl6\"]}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRpcv2CborRichDataPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/RichData",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "populated",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "omhzY2VuYXJpb2lwb3B1bGF0ZWRmcmVjb3JkpGdwYXlsb2Fkp2VsYWJlbGdmaXh0dXJlZWNoaWxko2RuYW1lZWNoaWxkZWNvdW50B2ZhY3RpdmX1ZWl0ZW1zgaNkbmFtZWVmaXJzdGVjb3VudAFmYWN0aXZl9GZsb29rdXChY2tleaJkbmFtZWZtYXBwZWRlY291bnQCZmNob2ljZaFkdGV4dGZjaG9zZW5lYnl0ZXNDYWJjaXRpbWVzdGFtcMH7AAAAAAAAAABoY2hpbGRyZW6BomRuYW1lZWV4dHJhZWNvdW50CWdjaG9pY2VzgqFkdGV4dGtsaXN0LWNob2ljZaFmcmVjb3JkomRuYW1lbHVuaW9uLXJlY29yZGVjb3VudANlYmxvYnOCQ2FiY0N4eXo=",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborRichDataError",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/RichData",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "error",
            "record": {
                "payload": {
                    "label": "fixture",
                    "child": {
                        "name": "child",
                        "count": 7,
                        "active": true
                    },
                    "items": [
                        {
                            "name": "first",
                            "count": 1,
                            "active": false
                        }
                    ],
                    "lookup": {
                        "key": {
                            "name": "mapped",
                            "count": 2
                        }
                    },
                    "choice": {
                        "text": "chosen"
                    },
                    "bytes": "abc",
                    "timestamp": 0
                },
                "children": [
                    {
                        "name": "extra",
                        "count": 9
                    }
                ],
                "choices": [
                    {
                        "text": "list-choice"
                    },
                    {
                        "record": {
                            "name": "union-record",
                            "count": 3
                        }
                    }
                ],
                "blobs": [
                    "abc",
                    "xyz"
                ]
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:error"
        ],
        "body": "omhzY2VuYXJpb2VlcnJvcmZyZWNvcmSkZ3BheWxvYWSnZWxhYmVsZ2ZpeHR1cmVlY2hpbGSjZG5hbWVlY2hpbGRlY291bnQHZmFjdGl2ZfVlaXRlbXOBo2RuYW1lZWZpcnN0ZWNvdW50AWZhY3RpdmX0Zmxvb2t1cKFja2V5omRuYW1lZm1hcHBlZGVjb3VudAJmY2hvaWNloWR0ZXh0ZmNob3NlbmVieXRlc0NhYmNpdGltZXN0YW1wwfsAAAAAAAAAAGhjaGlsZHJlboGiZG5hbWVlZXh0cmFlY291bnQJZ2Nob2ljZXOCoWR0ZXh0a2xpc3QtY2hvaWNloWZyZWNvcmSiZG5hbWVsdW5pb24tcmVjb3JkZWNvdW50A2VibG9ic4JDYWJjQ3h5eg==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ModeledDefaults @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonModeledDefaultsDefault",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "default"
        },
        "params": {
            "scenario": "default"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:default"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonModeledDefaultsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonModeledDefaultsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestxmlModeledDefaultsDefault",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "default"
        },
        "params": {
            "scenario": "default"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:default"
        ],
        "body": "",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlModeledDefaultsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedRestxmlModeledDefaultsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restXml",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "Content-Type": "application/xml",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "",
        "bodyMediaType": "application/xml"
    },
    {
        "id": "MultiSeedAwsjson10ModeledDefaultsDefault",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.ModeledDefaults"
        },
        "params": {
            "scenario": "default"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:default"
        ],
        "body": "{\"scenario\":\"default\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10ModeledDefaultsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.ModeledDefaults"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10ModeledDefaultsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.ModeledDefaults"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11ModeledDefaultsDefault",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.ModeledDefaults"
        },
        "params": {
            "scenario": "default"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:default"
        ],
        "body": "{\"scenario\":\"default\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11ModeledDefaultsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.ModeledDefaults"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11ModeledDefaultsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.ModeledDefaults"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRpcv2CborModeledDefaultsDefault",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/ModeledDefaults",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "default"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:default"
        ],
        "body": "oWhzY2VuYXJpb2dkZWZhdWx0",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborModeledDefaultsOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/ModeledDefaults",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "override"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "oWhzY2VuYXJpb2hvdmVycmlkZQ==",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborModeledDefaultsPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/ModeledDefaults",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "populated"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "oWhzY2VuYXJpb2lwb3B1bGF0ZWQ=",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#NullableValues @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonNullableValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/nullable",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "absent"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonNullableValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/nullable",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "empty"
        },
        "params": {
            "scenario": "empty",
            "values": [],
            "entries": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"values\":[],\"entries\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonNullableValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/nullable",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "values": [
                "first",
                null,
                "last"
            ],
            "entries": {
                "one": "value",
                "nil": null
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"values\":[\"first\",null,\"last\"],\"entries\":{\"one\":\"value\",\"nil\":null}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10NullableValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.NullableValues"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "{\"scenario\":\"absent\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10NullableValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.NullableValues"
        },
        "params": {
            "scenario": "empty",
            "values": [],
            "entries": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"scenario\":\"empty\",\"values\":[],\"entries\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10NullableValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.NullableValues"
        },
        "params": {
            "scenario": "populated",
            "values": [
                "first",
                null,
                "last"
            ],
            "entries": {
                "one": "value",
                "nil": null
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"values\":[\"first\",null,\"last\"],\"entries\":{\"one\":\"value\",\"nil\":null}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11NullableValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.NullableValues"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "{\"scenario\":\"absent\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11NullableValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.NullableValues"
        },
        "params": {
            "scenario": "empty",
            "values": [],
            "entries": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"scenario\":\"empty\",\"values\":[],\"entries\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11NullableValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.NullableValues"
        },
        "params": {
            "scenario": "populated",
            "values": [
                "first",
                null,
                "last"
            ],
            "entries": {
                "one": "value",
                "nil": null
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"values\":[\"first\",null,\"last\"],\"entries\":{\"one\":\"value\",\"nil\":null}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRpcv2CborNullableValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/NullableValues",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "oWhzY2VuYXJpb2ZhYnNlbnQ=",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborNullableValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/NullableValues",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "empty",
            "values": [],
            "entries": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "o2hzY2VuYXJpb2VlbXB0eWZ2YWx1ZXOAZ2VudHJpZXOg",
        "bodyMediaType": "application/cbor"
    },
    {
        "id": "MultiSeedRpcv2CborNullableValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "smithy.protocols#rpcv2Cbor",
        "method": "POST",
        "uri": "/service/MultiProtocolService/operation/NullableValues",
        "headers": {
            "Content-Type": "application/cbor",
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor"
        },
        "params": {
            "scenario": "populated",
            "values": [
                "first",
                null,
                "last"
            ],
            "entries": {
                "one": "value",
                "nil": null
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "o2hzY2VuYXJpb2lwb3B1bGF0ZWRmdmFsdWVzg2VmaXJzdPZkbGFzdGdlbnRyaWVzomNvbmVldmFsdWVjbmls9g==",
        "bodyMediaType": "application/cbor"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#DocumentValues @smithy.test#httpRequestTests(
[
    {
        "id": "MultiSeedRestjsonDocumentValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/document",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "absent"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonDocumentValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/document",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "empty"
        },
        "params": {
            "scenario": "empty",
            "document": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"document\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonDocumentValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/document",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "populated"
        },
        "params": {
            "scenario": "populated",
            "document": {
                "kind": "fixture",
                "nil": null,
                "count": 7
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"document\":{\"kind\":\"fixture\",\"nil\":null,\"count\":7}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedRestjsonDocumentValuesOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/document",
        "headers": {
            "Content-Type": "application/json",
            "x-scenario": "override"
        },
        "params": {
            "scenario": "override",
            "document": null
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"document\":null}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10DocumentValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "{\"scenario\":\"absent\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10DocumentValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "empty",
            "document": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"scenario\":\"empty\",\"document\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10DocumentValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "populated",
            "document": {
                "kind": "fixture",
                "nil": null,
                "count": 7
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"document\":{\"kind\":\"fixture\",\"nil\":null,\"count\":7}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson10DocumentValuesOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.0",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "override",
            "document": null
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\",\"document\":null}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11DocumentValuesAbsent",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "absent"
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:absent"
        ],
        "body": "{\"scenario\":\"absent\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11DocumentValuesEmpty",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "empty",
            "document": {}
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:empty"
        ],
        "body": "{\"scenario\":\"empty\",\"document\":{}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11DocumentValuesPopulated",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "populated",
            "document": {
                "kind": "fixture",
                "nil": null,
                "count": 7
            }
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:populated"
        ],
        "body": "{\"scenario\":\"populated\",\"document\":{\"kind\":\"fixture\",\"nil\":null,\"count\":7}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiSeedAwsjson11DocumentValuesOverride",
        "documentation": "Assert decoded input values and seed the tagged actual success/error handler scenario.",
        "protocol": "aws.protocols#awsJson1_1",
        "method": "POST",
        "uri": "/",
        "headers": {
            "Content-Type": "application/x-amz-json-1.1",
            "X-Amz-Target": "MultiProtocolService.DocumentValues"
        },
        "params": {
            "scenario": "override",
            "document": null
        },
        "appliesTo": "server",
        "tags": [
            "fuzz-request:override"
        ],
        "body": "{\"scenario\":\"override\",\"document\":null}",
        "bodyMediaType": "application/json"
    }
]
)

// END MULTIPROTOCOL SCENARIO REQUEST SEEDS
