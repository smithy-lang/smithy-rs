// Every matcher kind and path comparator, and failure acceptors with messages that evaluate
// to a string and to an array of strings.
$version: "2"

namespace ex

use smithy.waiters#waitable

service Service {
    version: "1"
    operations: [GetThing]
}

@waitable(
    ThingReady: {
        acceptors: [
            {
                state: "success"
                matcher: { output: { path: "status", expected: "ready", comparator: "stringEquals" } }
            }
            {
                state: "success"
                matcher: { output: { path: "ready", expected: "true", comparator: "booleanEquals" } }
            }
            {
                state: "success"
                matcher: { output: { path: "items[].status", expected: "ready", comparator: "allStringEquals" } }
            }
            {
                state: "retry"
                matcher: { output: { path: "items[].status", expected: "pending", comparator: "anyStringEquals" } }
            }
            {
                state: "success"
                matcher: {
                    inputOutput: { path: "input.name == output.name", expected: "true", comparator: "booleanEquals" }
                }
            }
            {
                state: "failure"
                matcher: { output: { path: "status", expected: "failed", comparator: "stringEquals" } }
                message: "reason"
            }
            {
                state: "failure"
                matcher: { output: { path: "items[].status", expected: "failed", comparator: "anyStringEquals" } }
                message: "reasons"
            }
            { state: "retry", matcher: { errorType: "NotFound" } }
            { state: "retry", matcher: { errorType: "ex#Throttled" } }
            { state: "failure", matcher: { success: false } }
            { state: "success", matcher: { success: true } }
        ]
    }
)
operation GetThing {
    input := {
        name: String
    }
    output := {
        name: String
        status: Status
        ready: Boolean
        reason: String
        reasons: Reasons
        items: Items
    }
    errors: [NotFound, Throttled]
}

enum Status {
    READY = "ready"
    PENDING = "pending"
    FAILED = "failed"
}

list Reasons {
    member: String
}

list Items {
    member: Item
}

structure Item {
    status: Status
}

@error("client")
structure NotFound {}

@error("client")
structure Throttled {}
