// Malformed trait values that the reference implementation cannot decode, which it reports
// while loading the model, and this crate reports while decoding the trait. Each operation's
// trait has one problem.
//
// The reference implementation does not validate a model that fails to load, so problems
// that only validation finds are in other fixtures.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable([])
operation NotAnObject {}

@waitable(W: "waiter")
operation WaiterNotAnObject {}

@waitable(W: { minDelay: 2 })
operation NoAcceptors {}

@waitable(W: { acceptors: {} })
operation AcceptorsNotAList {}

@waitable(W: { acceptors: ["success"] })
operation AcceptorNotAnObject {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], documentation: 1 })
operation DocumentationNotAString {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], minDelay: "2" })
operation DelayNotANumber {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], deprecated: "yes" })
operation DeprecatedNotABoolean {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], tags: "slow" })
operation TagsNotAList {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true } }], tags: ["slow", 1] })
operation TagNotAString {}

@waitable(W: { acceptors: [{ matcher: { success: true } }] })
operation NoState {}

@waitable(W: { acceptors: [{ state: "done", matcher: { success: true } }] })
operation UnknownState {}

@waitable(W: { acceptors: [{ state: "success" }] })
operation NoMatcher {}

@waitable(W: { acceptors: [{ state: "success", matcher: [] }] })
operation MatcherNotAnObject {}

@waitable(W: { acceptors: [{ state: "success", matcher: {} }] })
operation NoMatcherMember {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: true, errorType: "E" } }] })
operation TwoMatcherMembers {}

@waitable(W: { acceptors: [{ state: "success", matcher: { success: "true" } }] })
operation SuccessNotABoolean {}

@waitable(W: { acceptors: [{ state: "success", matcher: { errorType: 1 } }] })
operation ErrorTypeNotAString {}

@waitable(W: { acceptors: [{ state: "failure", matcher: { success: false }, message: 1 }] })
operation MessageNotAString {}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: "status" } }] })
operation PathMatcherNotAnObject {}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { expected: "b", comparator: "stringEquals" } } }] })
operation NoPath {}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "a", comparator: "stringEquals" } } }] })
operation NoExpected {}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "a", expected: "b" } } }] })
operation NoComparator {}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: "a", expected: "b", comparator: "equals" } } }] })
operation UnknownComparator {}

@waitable(W: { acceptors: [{ state: "success", matcher: { output: { path: 1, expected: "b", comparator: "stringEquals" } } }] })
operation PathNotAString {}
