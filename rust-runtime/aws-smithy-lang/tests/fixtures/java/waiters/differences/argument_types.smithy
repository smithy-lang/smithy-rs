// A function argument of the wrong type. The reference implementation reports this as an
// error; this crate's static analysis only reports errors for problems that do not depend on
// the types it infers, so it reports a warning.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "length(count) > `0`", expected: "true", comparator: "booleanEquals" } } }
        ]
    }
)
operation GetThing {
    output := {
        count: Integer
    }
}
