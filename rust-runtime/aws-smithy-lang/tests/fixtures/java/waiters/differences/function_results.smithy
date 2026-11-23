// Functions whose results the reference implementation's linter gets wrong, so that it
// reports DANGER events for valid expressions. This crate reports nothing:
//
// - `EndsWith`: `ends_with` returns a boolean, not a number.
// - `MaxBy`: `max_by` returns the element with the largest key, not the key.
// - `SortBy`: `sort_by` returns the sorted elements, so `[0]` selects an element.
// - `Merge`: `merge` returns an object with the members of every argument.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "ends_with(name, 'z')", expected: "true", comparator: "booleanEquals" } } }
        ]
    }
)
operation EndsWith {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "max_by(items, &count).name", expected: "a", comparator: "stringEquals" } } }
        ]
    }
)
operation MaxBy {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "sort_by(items, &name)[0].name", expected: "a", comparator: "stringEquals" } } }
        ]
    }
)
operation SortBy {
    output: Thing
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { output: { path: "merge(labels, `{\"env\": \"prod\"}`).env", expected: "prod", comparator: "stringEquals" } } }
        ]
    }
)
operation Merge {
    output: Thing
}

structure Thing {
    name: String
    labels: Labels
    items: Items
}

map Labels {
    key: String
    value: String
}

list Items {
    member: Item
}

structure Item {
    name: String
    count: Integer
}
