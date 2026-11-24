// A message whose result type cannot be determined statically: a member of a document. The
// reference implementation rejects every message not known to be a string or an array of
// strings; this crate only rejects messages known not to be.
$version: "2"

namespace ex

use smithy.waiters#waitable

@waitable(
    W: {
        acceptors: [
            { state: "success", matcher: { success: true } }
            { state: "failure", matcher: { success: false }, message: "details.reason" }
        ]
    }
)
operation GetThing {
    output := {
        details: Document
    }
}
