// Rules between the values of a waiter or acceptor. Each operation's trait has one problem.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], minDelay: 10, maxDelay: 5 })
operation MinDelayAboveMaxDelay {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], minDelay: 121 })
operation MinDelayAboveDefaultMaxDelay {}

@waitable(
    W: {
        acceptors: [
            { state: "retry", matcher: { success: false } }
            { state: "failure", matcher: { success: true } }
        ]
    }
)
operation NoSuccessState {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true }, message: "name" }] })
operation MessageOnSuccess {
    output := {
        name: String
    }
}

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "retry", matcher: { success: false }, message: "name" }
        ]
    }
)
operation MessageOnRetry {
    output := {
        name: String
    }
}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "ready", expected: "yes", comparator: "booleanEquals" } } }] })
operation BooleanExpectsNeither {
    output := {
        ready: Boolean
    }
}
