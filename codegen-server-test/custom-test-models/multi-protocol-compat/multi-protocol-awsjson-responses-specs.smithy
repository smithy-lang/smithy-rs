$version: "2"

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

namespace smithy.rust.codegen.test.multiprotocol

// Import with responseBindings: "smithy-specs" for both AWS JSON protocols.

apply smithy.rust.codegen.test.multiprotocol#StructurePayload @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10StructurePayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"body\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson10StructurePayloadEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"body\":{\"label\":\"empty\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11StructurePayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"body\":{\"label\":\"fixture\",\"child\":{\"name\":\"child\",\"count\":7,\"active\":true},\"items\":[{\"name\":\"first\",\"count\":1,\"active\":false}],\"lookup\":{\"key\":{\"name\":\"mapped\",\"count\":2}},\"choice\":{\"text\":\"chosen\"},\"bytes\":\"YWJj\",\"timestamp\":0}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11StructurePayloadEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"body\":{\"label\":\"empty\"}}",
        "bodyMediaType": "application/json"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#UnionPayload @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10UnionPayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"body\":{\"text\":\"chosen\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson10UnionPayloadOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"body\":{\"record\":{\"name\":\"union-record\",\"count\":3}}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11UnionPayloadPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"body\":{\"text\":\"chosen\"}}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11UnionPayloadOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"body\":{\"record\":{\"name\":\"union-record\",\"count\":3}}}",
        "bodyMediaType": "application/json"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ResponseBindings @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10ResponseBindingsAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson10ResponseBindingsPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":299}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson10ResponseBindingsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":202}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11ResponseBindingsAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11ResponseBindingsPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":299}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11ResponseBindingsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":202}",
        "bodyMediaType": "application/json"
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#RichData @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10RichDataPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson11RichDataPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ModeledDefaults @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10ModeledDefaultsDefault",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson10ModeledDefaultsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson11ModeledDefaultsDefault",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson11ModeledDefaultsOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#NullableValues @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10NullableValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson10NullableValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson10NullableValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson11NullableValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson11NullableValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson11NullableValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#DocumentValues @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10DocumentValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson10DocumentValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson10DocumentValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "id": "MultiAwsjson10DocumentValuesOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
    },
    {
        "id": "MultiAwsjson11DocumentValuesAbsent",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson11DocumentValuesEmpty",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson11DocumentValuesPopulated",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson11DocumentValuesOverride",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 200,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "id": "MultiAwsjson10RichFailureError",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 400,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
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
        "body": "{\"message\":\"rejected\",\"details\":{\"name\":\"diagnostic\",\"count\":9,\"active\":false},\"etag\":\"error-tag\",\"metadata\":{\"reason\":\"fixture\"},\"__type\":\"smithy.rust.codegen.test.multiprotocol#RichFailure\"}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "MultiAwsjson11RichFailureError",
        "documentation": "Compare independently specified status, response bindings, and complete populated body.",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 400,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
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
        "body": "{\"message\":\"rejected\",\"details\":{\"name\":\"diagnostic\",\"count\":9,\"active\":false},\"etag\":\"error-tag\",\"metadata\":{\"reason\":\"fixture\"},\"__type\":\"RichFailure\"}",
        "bodyMediaType": "application/json"
    }
]
)
