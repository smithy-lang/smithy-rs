$version: "2.0"

// Selects the upstream REST XML operations whose protocol cases explicitly apply to servers.
// The server projections generate and run those request/response assertions while avoiding
// client-only cases that do not describe the server contract.

namespace smithy.rust.server.protocoltests

use aws.api#service
use aws.protocols#restXml
use aws.protocoltests.restxml#ContentTypeParameters
use aws.protocoltests.restxml#HttpEmptyPrefixHeaders
use aws.protocoltests.restxml#NullAndEmptyHeadersServer
use aws.protocoltests.restxml#QueryParamsAsStringListMap
use aws.protocoltests.restxml#QueryPrecedence

/// The subset of the upstream RestXml protocol test service whose tests are
/// explicitly marked as applying to servers.
@service(sdkId: "Rest Xml Server Protocol Tests")
@restXml
service RestXmlServerTests {
    version: "2026-09-11"
    operations: [
        ContentTypeParameters
        NullAndEmptyHeadersServer
        HttpEmptyPrefixHeaders
        QueryPrecedence
        QueryParamsAsStringListMap
    ]
}
