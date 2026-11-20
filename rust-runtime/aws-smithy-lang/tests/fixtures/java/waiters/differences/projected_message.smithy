// A message that projects an array of strings. To find the type of an array's elements, the
// reference implementation appends `[0]` to the expression. Appended to a projection, the
// index applies to each projected value instead of the projected array, so the result is
// still an array, and the reference implementation rejects the message as an array of
// arrays. This crate accepts it.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { success: false }, message: "items[].reason" }
        ]
    }
)
operation GetThing {
    output := {
        items: Items
    }
}

list Items {
    member: Item
}

structure Item {
    reason: String
}
