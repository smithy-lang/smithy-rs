$version: "1.0"
namespace crate

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use aws.protocols#awsJson1_1
use aws.api#service

/// Confounds model generation machinery with lots of problematic names
@awsJson1_1
@service(sdkId: "Config")
service Config {
    version: "2006-03-01",
    operations: [
       ReservedWordsAsMembers,
       StructureNamePunning,
       ErrCollisions
    ]
}

@httpRequestTests([
    {
        id: "reserved_words",
        protocol: awsJson1_1,
        params: {
            "as": 5,
            "async": true,
        },
        method: "POST",
        uri: "/",
        body: "{\"as\": 5, \"async\": true}",
        bodyMediaType: "application/json"
    }
])
operation ReservedWordsAsMembers {
    input: ReservedWords
}


structure ReservedWords {
    as: Integer,
    async: Boolean,
    enum: UnknownVariantCollidingEnum,
    self: Boolean,
    crate: Boolean,
    super: Boolean
}

@httpRequestTests([
    {
        id: "structure_punning",
        protocol: awsJson1_1,
        params: {
            "regular_string": "hello!",
        },
        method: "POST",
        uri: "/",
        body: "{\"regular_string\": \"hello!\"}",
        bodyMediaType: "application/json"
    }
])
operation StructureNamePunning {
    input: StructureNamePunningInput
}

structure StructureNamePunningInput {
    regular_string: smithy.api#String,
    punned_vec: Vec
}

structure Vec {
    pv_member: Boolean
}


operation ErrCollisions {
    errors: [
        CollidingError,
        CollidingException
        // , ErrCollisionsException
    ]
}

@error("client")
structure CollidingError {

}

/// This will be renamed to CollidingError
@error("client")
structure CollidingException {

}

// This will conflict with the name of the top-level error declaration
// Fixing this is more refactoring than I want to get into right now
// @error("client")
// structure ErrCollisionsException { }

// The "Unknown" value on this enum collides with the code generated "Unknown" variant used for backwards compatibility
@enum([
    { name: "Known", value: "Known" },
    { name: "Unknown", value: "Unknown" },
    { name: "Self", value: "Self" },
    { name: "UnknownValue", value: "UnknownValue" },
    { name: "SelfValue", value: "SelfValue" }
])
string UnknownVariantCollidingEnum
