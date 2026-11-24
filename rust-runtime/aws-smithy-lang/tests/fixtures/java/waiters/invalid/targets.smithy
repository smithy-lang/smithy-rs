// The trait can only be applied to operations whose input and output have no event streams.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }] })
structure NotAnOperation {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation InputEventStream {
    input := {
        events: Events
    }
}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation OutputEventStream {
    output := {
        events: Events
    }
}

@streaming
union Events {
    event: Event
}

structure Event {}
