// Two waiters of one operation whose names differ only by case. The specification requires
// names to be case-insensitively unique in a service's closure; the reference implementation
// only compares the names of different operations.
$version: "2"

namespace ex

use smithy.waiters#waitable

service Service {
    version: "1"
    operations: [GetThing]
}

@waitable(
    ThingReady: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
    Thingready: {
        acceptors: [{ state: "success", matcher: { success: true } }]
    }
)
operation GetThing {}
