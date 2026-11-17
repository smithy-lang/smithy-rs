// Invalid JMESPath expressions, and messages whose result is known not to be a string or an
// array of strings. Each operation's trait has one problem.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "items[", expected: "a", comparator: "stringEquals" } } }] })
operation PathSyntax {
    output: Thing
}

@waitable(W: { acceptors: [{ state: "success", matcher: { inputOutput: { path: "input.", expected: "a", comparator: "stringEquals" } } }] })
operation InputOutputPathSyntax {
    output: Thing
}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "", expected: "a", comparator: "stringEquals" } } }] })
operation EmptyPath {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { success: false }, message: "length(name" }
        ]
    }
)
operation MessageSyntax {
    output: Thing
}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "lenght(items) > `0`", expected: "true", comparator: "booleanEquals" } } }] })
operation UnknownFunction {
    output: Thing
}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "length(items, name) > `0`", expected: "true", comparator: "booleanEquals" } } }] })
operation TooManyArguments {
    output: Thing
}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "contains(ids)", expected: "true", comparator: "booleanEquals" } } }] })
operation TooFewArguments {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { success: false }, message: "count" }
        ]
    }
)
operation MessageIsANumber {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { success: false }, message: "items" }
        ]
    }
)
operation MessageIsAnArrayOfObjects {
    output: Thing
}

// Both implementations also warn that `nmae` is not a member.
@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { success: false }, message: "nmae" }
        ]
    }
)
operation MessageIsNull {
    output: Thing
}

structure Thing {
    name: String
    count: Integer
    ids: Ids
    items: Items
}

list Ids {
    member: String
}

list Items {
    member: Item
}

structure Item {
    id: String
}
