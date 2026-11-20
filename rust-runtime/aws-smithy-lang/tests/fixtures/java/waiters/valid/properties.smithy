// Waiter properties:
//
// - `GetThing` has waiters out of alphabetical order. `Zeta` omits every optional property,
//   `Alpha` sets each to its default, and `Mid` sets each to another value, including the
//   smallest and largest delays.
// - `GetOther` has unknown waiter, acceptor, and path matcher properties, which both
//   implementations ignore with a warning.
$version: "2"

namespace ex

use smithy.waiters#waitable

service Service {
    version: "1"
    operations: [GetThing, GetOther]
}

@waitable(
    Zeta: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
    Alpha: {
        acceptors: [{ state: "success", matcher: { success: true } }]
        minDelay: 2
        maxDelay: 120
        deprecated: false
        tags: []
    }
    Mid: {
        documentation: "Waits until the *thing* exists."
        acceptors: [{ state: "success", matcher: { success: true } }]
        minDelay: 1
        maxDelay: 2147483647
        deprecated: true
        tags: ["slow", "legacy"]
    }
)
operation GetThing {}

@waitable(
    OtherReady: {
        acceptors: [
            {
                state: "success"
                matcher: { output: { path: "name", expected: "x", comparator: "stringEquals", extra: 1 } }
                note: "ignored"
            }
        ]
        future: { enabled: true }
    }
)
operation GetOther {
    output := {
        name: String
    }
}
