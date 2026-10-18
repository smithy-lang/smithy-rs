//! Review finding: the upgrade layer answers an operation/selection mismatch with a bare 500.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use protocol_compat::tampered;
use support::*;

#[tokio::test]
async fn mismatched_or_missing_operation_selection_response_is_pinned() {
    let service = tampered::service();
    // Records the GetWidget selection.
    let ok = send(service.clone(), rest("GET", "/widgets/1", &[], b"")).await;
    assert_eq!(ok.status, 200);

    for (what, tamper) in [("mismatched selection", "swap"), ("missing selection", "drop")] {
        let reply = send(service.clone(), rest("POST", "/ping", &[("x-tamper", tamper)], b"")).await;
        // Pinned as observed: a bare 500, not restJson1's framed InternalFailure (`x-amzn-errortype`, JSON body).
        assert_eq!((reply.status, reply.headers.len(), reply.body.len()), (500, 0, 0), "{what}");
    }
}
