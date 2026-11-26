// Waiter names must be case-insensitively unique within a service's closure. `GetThing`,
// `ListThings` (bound through the `Thing` resource), and `GetPart` (bound through the nested
// `Part` resource) have names that differ only by case. `Plain`'s name is unique.
$version: "2"

namespace ex

use smithy.waiters#waitable

service Service {
    version: "1"
    operations: [GetThing, Plain]
    resources: [Thing]
}

resource Thing {
    operations: [ListThings]
    resources: [Part]
}

resource Part {
    operations: [GetPart]
}

@waitable(ThingReady: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation GetThing {}

@waitable(THINGREADY: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation ListThings {}

@waitable(ThingREADY: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation GetPart {}

@waitable(Unique: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation Plain {}
