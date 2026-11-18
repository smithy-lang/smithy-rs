$version: "2"

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

namespace smithy.rust.codegen.test.multiprotocol

// Import with historical responseBindings (the default). See README.md for legacy observations.

apply smithy.rust.codegen.test.multiprotocol#StructurePayload @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10StructurePayloadPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10StructurePayloadEmpty",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11StructurePayloadPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11StructurePayloadEmpty",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#UnionPayload @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10UnionPayloadPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10UnionPayloadOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11UnionPayloadPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11UnionPayloadOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ResponseBindings @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10ResponseBindingsAbsent",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.0",
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10ResponseBindingsPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.0",
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":299}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10ResponseBindingsOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.0",
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":202}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11ResponseBindingsAbsent",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.1",
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\"}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11ResponseBindingsPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.1",
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":299}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11ResponseBindingsOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.1",
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
        "body": "{\"name\":\"n\",\"etag\":\"\\\"abc\\\"\",\"metadata\":{\"one\":\"1\",\"two\":\"2\"},\"code\":202}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#RichData @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10RichDataPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11RichDataPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#ModeledDefaults @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10ModeledDefaultsDefault",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10ModeledDefaultsOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11ModeledDefaultsDefault",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11ModeledDefaultsOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#NullableValues @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10NullableValuesAbsent",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10NullableValuesEmpty",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10NullableValuesPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11NullableValuesAbsent",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11NullableValuesEmpty",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11NullableValuesPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#DocumentValues @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10DocumentValuesAbsent",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10DocumentValuesEmpty",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10DocumentValuesPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson10DocumentValuesOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11DocumentValuesAbsent",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
        "headers": {
            "content-type": "application/x-amz-json-1.1"
        },
        "params": {},
        "appliesTo": "server",
        "tags": [
            "fuzz:absent"
        ],
        "body": "{}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11DocumentValuesEmpty",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11DocumentValuesPopulated",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11DocumentValuesOverride",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 201,
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
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)

apply smithy.rust.codegen.test.multiprotocol#RichFailure @smithy.test#httpResponseTests(
[
    {
        "id": "MultiAwsjson10RichFailureError",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 422,
        "headers": {
            "content-type": "application/x-amz-json-1.0",
            "x-error-meta-reason": "fixture",
            "x-error-tag": "error-tag"
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
        "body": "{\"message\":\"rejected\",\"details\":{\"name\":\"diagnostic\",\"count\":9,\"active\":false},\"etag\":\"error-tag\",\"metadata\":{\"reason\":\"fixture\"},\"__type\":\"smithy.rust.codegen.test.multiprotocol#RichFailure\"}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    },
    {
        "id": "MultiAwsjson11RichFailureError",
        "documentation": "Historical AWS JSON response observed with generated serde (schemaSerde disabled).",
        "protocol": "aws.protocols#awsJson1_1",
        "code": 422,
        "headers": {
            "content-type": "application/x-amz-json-1.1",
            "x-error-meta-reason": "fixture",
            "x-error-tag": "error-tag"
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
        "body": "{\"message\":\"rejected\",\"details\":{\"name\":\"diagnostic\",\"count\":9,\"active\":false},\"etag\":\"error-tag\",\"metadata\":{\"reason\":\"fixture\"},\"__type\":\"RichFailure\"}",
        "bodyMediaType": "application/json",
        "requireHeaders": [
            "content-length"
        ]
    }
]
)
