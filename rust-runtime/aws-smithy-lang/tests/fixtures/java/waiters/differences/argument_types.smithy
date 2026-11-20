// Function arguments of the wrong type whose sample in the reference implementation might have
// another type. This crate reports an error only where the reference implementation's sample
// is certain to have the wrong type, and warns otherwise. Both paths select from `Node` more
// than once, and the reference implementation samples a member it visits a second time as a
// value of any type. `GetNext`'s `next.count` still samples as a number, so the reference
// implementation reports an error. In `GetNextNext`, the second `next` is a second visit, so
// it reports nothing.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "length(next.count) > `0`", expected: "true", comparator: "booleanEquals" } } }
        ]
    }
)
operation GetNext {
    output: Node
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "length(next.next.count) > `0`", expected: "true", comparator: "booleanEquals" } } }
        ]
    }
)
operation GetNextNext {
    output: Node
}

structure Node {
    next: Node
    count: Integer
}
