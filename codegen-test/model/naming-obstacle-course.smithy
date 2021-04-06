$version: "1.0"
namespace crate

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use aws.protocols#awsJson1_1

/// Confounds model generation machinery with lots of problematic names
@awsJson1_1
service Config {
    version: "2006-03-01",
    operations: [
       ReservedWordsAsMembers,
       StructureNamePunning,
       ErrCollisions,
       Result,
       Option
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
    async: Boolean
}

@httpRequestTests([
    {
        id: "structure_punning",
        protocol: awsJson1_1,
        params: {
            "regular_string": "hello!",
            "punned_string": { "ps_member": true },
        },
        method: "POST",
        uri: "/",
        body: "{\"regular_string\": \"hello!\", \"punned_string\": { \"ps_member\": true }}",
        bodyMediaType: "application/json"
    }
])
operation StructureNamePunning {
    input: StructureNamePunningInput
}

structure StructureNamePunningInput {
    regular_string: smithy.api#String,
    punned_string: crate#String,
    punned_vec: Vec,
}


structure Vec {
    pv_member: Boolean
}

structure String {
    ps_member: Boolean
}

operation ErrCollisions {
    errors: [
        CollidingError,
        CollidingException
        // , ErrCollisionsException
    ]
}

operation Result {
    input: Vec
}

operation Option {
    input: Vec
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
