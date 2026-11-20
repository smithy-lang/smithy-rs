// Waiter names only need to be unique within each service's closure:
//
// - `Service` reaches `GetThing` directly, `ListThings` through the `Thing` resource, and
//   `GetPart` through the nested `Part` resource. Their waiter names differ.
// - `Other` also contains `GetThing`, and has its own `ThingReady` waiter on `GetOther`. That
//   name is also on `Unbound`, which no service contains.
// - `Shared` is in both services.
// - `Plain` has no waiters, and `Empty` has no input or output.
$version: "2"

namespace ex

use smithy.waiters#waitable

service Service {
    version: "1"
    operations: [GetThing, Shared, Plain, Empty]
    resources: [Thing]
}

service Other {
    version: "1"
    operations: [GetThing, GetOther, Shared]
}

resource Thing {
    operations: [ListThings]
    resources: [Part]
}

resource Part {
    operations: [GetPart]
}

@waitable(
    ThingExists: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation GetThing {}

@waitable(
    ThingsListed: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation ListThings {}

@waitable(
    PartExists: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation GetPart {}

@waitable(
    ThingReady: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation GetOther {}

@waitable(
    ThingReady: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation Unbound {}

@waitable(
    SharedReady: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation Shared {}

operation Plain {}

@waitable(
    EmptyReady: {
        acceptors: [
            {
                state: "success"
                matcher: { inputOutput: { path: "input == output", expected: "true", comparator: "booleanEquals" } }
            }
        ]
    }
)
operation Empty {}
