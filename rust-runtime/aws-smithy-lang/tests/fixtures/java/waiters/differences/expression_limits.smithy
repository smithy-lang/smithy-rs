// Expressions beyond this crate's size limits, which it rejects without parsing so that model
// content cannot exhaust the stack of the recursive JMESPath parser: `Long`'s path is 2052
// bytes long, more than 2048, and `Nested`'s nests 33 levels deep, more than 32. The
// reference implementation has no limits.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name || name", expected: "a", comparator: "stringEquals" } } }
        ]
    }
)
operation Long {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "(((((((((((((((((((((((((((((((((name)))))))))))))))))))))))))))))))))", expected: "a", comparator: "stringEquals" } } }
        ]
    }
)
operation Nested {
    output: Thing
}

structure Thing {
    name: String
}
