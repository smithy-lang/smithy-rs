// Problems that the reference implementation reports as warnings, or as DANGER, which this
// crate reports as warnings. Neither rejects the model. Each operation has one problem. No
// service contains the operations, so their waiter names do not conflict.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "retry", matcher: { errorType: "ThrottlingException" } }
        ]
    }
)
operation UnmodeledErrorName {
    errors: [NotFound]
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "retry", matcher: { errorType: "other#NotFound" } }
        ]
    }
)
operation UnmodeledErrorId {
    errors: [NotFound]
}

@waitable(
    W: {
        acceptors: [{ state: "success", matcher: { output: { path: "count", expected: "1", comparator: "stringEquals" } } }]
    }
)
operation StringComparesNumber {
    output: Thing
}

@waitable(
    W: {
        acceptors: [{ state: "success", matcher: { output: { path: "name", expected: "true", comparator: "booleanEquals" } } }]
    }
)
operation BooleanComparesString {
    output: Thing
}

@waitable(
    W: {
        acceptors: [{ state: "success", matcher: { output: { path: "name", expected: "a", comparator: "allStringEquals" } } }]
    }
)
operation ArrayComparesString {
    output: Thing
}

@waitable(
    W: {
        acceptors: [{ state: "success", matcher: { output: { path: "nmae", expected: "a", comparator: "stringEquals" } } }]
    }
)
operation MissingMember {
    output: Thing
}

@waitable(
    W: {
        acceptors: [{ state: "success", matcher: { output: { path: "items[].ready", expected: "a", comparator: "anyStringEquals" } } }]
    }
)
operation MissingProjectedMember {
    output: Thing
}

@waitable(
    W: {
        acceptors: [{ state: "success", matcher: { inputOutput: { path: "name", expected: "a", comparator: "stringEquals" } } }]
    }
)
operation MissingInputOutputField {
    output: Thing
}

structure Thing {
    name: String
    count: Integer
    items: Items
}

list Items {
    member: Item
}

structure Item {
    id: String
}

@error("client")
structure NotFound {}
