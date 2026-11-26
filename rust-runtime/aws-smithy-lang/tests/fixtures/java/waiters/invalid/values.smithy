// Trait values that the reference implementation decodes, but that violate the constraints
// of the trait's definition. This crate reports them while decoding the trait. Each
// operation's trait has one problem.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable({})
operation NoWaiters {}

@waitable(thingReady: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation LowercaseName {}

@waitable(Thing_Ready: { acceptors: [{ state: "success", matcher: { success: true } }] })
operation UnderscoreName {}

@waitable(W: { acceptors: [] })
operation EmptyAcceptors {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], minDelay: 0 })
operation ZeroDelay {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], maxDelay: -1 })
operation NegativeDelay {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], minDelay: 1.5 })
operation FractionalDelay {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], maxDelay: 2147483648 })
operation DelayOutOfRange {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], tags: [""] })
operation EmptyTag {}

@waitable(W: { acceptors: [{ state: "success", matcher: { outputPath: { path: "a", expected: "b", comparator: "stringEquals" } } }] })
operation UnknownMatcherMember {}
