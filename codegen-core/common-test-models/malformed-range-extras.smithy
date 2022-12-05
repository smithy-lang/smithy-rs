$version: "2.0"

namespace aws.protocoltests.extras.restjson.validation

use aws.api#service
use aws.protocols#restJson1
use smithy.test#httpMalformedRequestTests
use smithy.framework#ValidationException

/// A REST JSON service that sends JSON requests and responses with validation applied
@service(sdkId: "Rest Json Validation Protocol")
@restJson1
service MalformedRangeValidation {
    version: "2022-11-23",
    operations: [
        MalformedRange,
        MalformedRangeOverride,
    ]
}

@suppress(["UnstableTrait"])
@http(uri: "/MalformedRange", method: "POST")
operation MalformedRange {
    input: MalformedRangeInput,
    errors: [ValidationException]
}

@suppress(["UnstableTrait"])
@http(uri: "/MalformedRangeOverride", method: "POST")
operation MalformedRangeOverride {
    input: MalformedRangeOverrideInput,
    errors: [ValidationException]
}

apply MalformedRange @httpMalformedRequestTests([
    {
        id: "RestJsonMalformedRangeShort",
        documentation: """
        When a short member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "short" : $value:L }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value $value:L at '/short' failed to satisfy constraint: Member must be between 2 and 8, inclusive",
                      "fieldList" : [{"message": "Value $value:L at '/short' failed to satisfy constraint: Member must be between 2 and 8, inclusive", "path": "/short"}]}"""
                }
            }
        },
        testParameters: {
            value: ["1", "9"]
        }
    },
    {
        id: "RestJsonMalformedRangeMinShort",
        documentation: """
        When a short member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "minShort" : 1 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 1 at '/minShort' failed to satisfy constraint: Member must be greater than or equal to 2",
                      "fieldList" : [{"message": "Value 1 at '/minShort' failed to satisfy constraint: Member must be greater than or equal to 2", "path": "/minShort"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeMaxShort",
        documentation: """
        When a short member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "maxShort" : 9 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 9 at '/maxShort' failed to satisfy constraint: Member must be less than or equal to 8",
                      "fieldList" : [{"message": "Value 9 at '/maxShort' failed to satisfy constraint: Member must be less than or equal to 8", "path": "/maxShort"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeInteger",
        documentation: """
        When a integer member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "integer" : $value:L }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value $value:L at '/integer' failed to satisfy constraint: Member must be between 2 and 8, inclusive",
                      "fieldList" : [{"message": "Value $value:L at '/integer' failed to satisfy constraint: Member must be between 2 and 8, inclusive", "path": "/integer"}]}"""
                }
            }
        },
        testParameters: {
            value: ["1", "9"]
        }
    },
    {
        id: "RestJsonMalformedRangeMinInteger",
        documentation: """
        When a integer member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "minInteger" : 1 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 1 at '/minInteger' failed to satisfy constraint: Member must be greater than or equal to 2",
                      "fieldList" : [{"message": "Value 1 at '/minInteger' failed to satisfy constraint: Member must be greater than or equal to 2", "path": "/minInteger"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeMaxInteger",
        documentation: """
        When a integer member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "maxInteger" : 9 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 9 at '/maxInteger' failed to satisfy constraint: Member must be less than or equal to 8",
                      "fieldList" : [{"message": "Value 9 at '/maxInteger' failed to satisfy constraint: Member must be less than or equal to 8", "path": "/maxInteger"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeLong",
        documentation: """
        When a long member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "long" : $value:L }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value $value:L at '/long' failed to satisfy constraint: Member must be between 2 and 8, inclusive",
                      "fieldList" : [{"message": "Value $value:L at '/long' failed to satisfy constraint: Member must be between 2 and 8, inclusive", "path": "/long"}]}"""
                }
            }
        },
        testParameters: {
            value: ["1", "9"]
        }
    },
    {
        id: "RestJsonMalformedRangeMinLong",
        documentation: """
        When a long member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "minLong" : 1 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 1 at '/minLong' failed to satisfy constraint: Member must be greater than or equal to 2",
                      "fieldList" : [{"message": "Value 1 at '/minLong' failed to satisfy constraint: Member must be greater than or equal to 2", "path": "/minLong"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeMaxLong",
        documentation: """
        When a long member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRange",
            body: """
            { "maxLong" : 9 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 9 at '/maxLong' failed to satisfy constraint: Member must be less than or equal to 8",
                      "fieldList" : [{"message": "Value 9 at '/maxLong' failed to satisfy constraint: Member must be less than or equal to 8", "path": "/maxLong"}]}"""
                }
            }
        }
    },
])

// now repeat the above tests, but for the more specific constraints applied to the input member
apply MalformedRangeOverride @httpMalformedRequestTests([
    {
        id: "RestJsonMalformedRangeShortOverride",
        documentation: """
        When a short member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "short" : $value:L }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value $value:L at '/short' failed to satisfy constraint: Member must be between 4 and 6, inclusive",
                      "fieldList" : [{"message": "Value $value:L at '/short' failed to satisfy constraint: Member must be between 4 and 6, inclusive", "path": "/short"}]}"""
                }
            }
        },
        testParameters: {
            value: ["3", "7"]
        }
    },
    {
        id: "RestJsonMalformedRangeMinShortOverride",
        documentation: """
        When a short member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "minShort" : 3 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 3 at '/minShort' failed to satisfy constraint: Member must be greater than or equal to 4",
                      "fieldList" : [{"message": "Value 3 at '/minShort' failed to satisfy constraint: Member must be greater than or equal to 4", "path": "/minShort"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeMaxShortOverride",
        documentation: """
        When a short member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "maxShort" : 7 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 7 at '/maxShort' failed to satisfy constraint: Member must be less than or equal to 6",
                      "fieldList" : [{"message": "Value 7 at '/maxShort' failed to satisfy constraint: Member must be less than or equal to 6", "path": "/maxShort"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeIntegerOverride",
        documentation: """
        When a integer member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "integer" : $value:L }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value $value:L at '/integer' failed to satisfy constraint: Member must be between 4 and 6, inclusive",
                      "fieldList" : [{"message": "Value $value:L at '/integer' failed to satisfy constraint: Member must be between 4 and 6, inclusive", "path": "/integer"}]}"""
                }
            }
        },
        testParameters: {
            value: ["3", "7"]
        }
    },
    {
        id: "RestJsonMalformedRangeMinIntegerOverride",
        documentation: """
        When a integer member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "minInteger" : 3 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 3 at '/minInteger' failed to satisfy constraint: Member must be greater than or equal to 4",
                      "fieldList" : [{"message": "Value 3 at '/minInteger' failed to satisfy constraint: Member must be greater than or equal to 4", "path": "/minInteger"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeMaxIntegerOverride",
        documentation: """
        When a integer member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "maxInteger" : 7 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 7 at '/maxInteger' failed to satisfy constraint: Member must be less than or equal to 6",
                      "fieldList" : [{"message": "Value 7 at '/maxInteger' failed to satisfy constraint: Member must be less than or equal to 6", "path": "/maxInteger"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeLongOverride",
        documentation: """
        When a long member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "long" : $value:L }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value $value:L at '/long' failed to satisfy constraint: Member must be between 4 and 6, inclusive",
                      "fieldList" : [{"message": "Value $value:L at '/long' failed to satisfy constraint: Member must be between 4 and 6, inclusive", "path": "/long"}]}"""
                }
            }
        },
        testParameters: {
            value: ["3", "7"]
        }
    },
    {
        id: "RestJsonMalformedRangeMinLongOverride",
        documentation: """
        When a long member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "minLong" : 3 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 3 at '/minLong' failed to satisfy constraint: Member must be greater than or equal to 4",
                      "fieldList" : [{"message": "Value 3 at '/minLong' failed to satisfy constraint: Member must be greater than or equal to 4", "path": "/minLong"}]}"""
                }
            }
        }
    },
    {
        id: "RestJsonMalformedRangeMaxLongOverride",
        documentation: """
        When a long member does not fit within range bounds,
        the response should be a 400 ValidationException.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedRangeOverride",
            body: """
            { "maxLong" : 7 }""",
            headers: {
                "content-type": "application/json"
            }
        },
        response: {
            code: 400,
            headers: {
                "x-amzn-errortype": "ValidationException"
            },
            body: {
                mediaType: "application/json",
                assertion: {
                    contents: """
                    { "message" : "1 validation error detected. Value 7 at '/maxLong' failed to satisfy constraint: Member must be less than or equal to 6",
                      "fieldList" : [{"message": "Value 7 at '/maxLong' failed to satisfy constraint: Member must be less than or equal to 6", "path": "/maxLong"}]}"""
                }
            }
        }
    },
])

structure MalformedRangeInput {
    short: RangeShort,
    minShort: MinShort,
    maxShort: MaxShort,

    integer: RangeInteger,
    minInteger: MinInteger,
    maxInteger: MaxInteger,

    long: RangeLong,
    minLong: MinLong,
    maxLong: MaxLong,
}

structure MalformedRangeOverrideInput {
    @range(min: 4, max: 6)
    short: RangeShort,
    @range(min: 4)
    minShort: MinShort,
    @range(max: 6)
    maxShort: MaxShort,

    @range(min: 4, max: 6)
    integer: RangeInteger,
    @range(min: 4)
    minInteger: MinInteger,
    @range(max: 6)
    maxInteger: MaxInteger,

    @range(min: 4, max: 6)
    long: RangeLong,
    @range(min: 4)
    minLong: MinLong,
    @range(max: 6)
    maxLong: MaxLong,
}

@range(min: 2, max: 8)
short RangeShort

@range(min: 2)
short MinShort

@range(max: 8)
short MaxShort

@range(min: 2, max: 8)
integer RangeInteger

@range(min: 2)
integer MinInteger

@range(max: 8)
integer MaxInteger

@range(min: 2, max: 8)
long RangeLong

@range(min: 2)
long MinLong

@range(max: 8)
long MaxLong
