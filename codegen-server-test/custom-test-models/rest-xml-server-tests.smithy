$version: "2.0"

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
