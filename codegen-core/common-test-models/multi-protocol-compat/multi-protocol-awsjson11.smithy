$version: "2"

// A client of the multi-protocol service speaking awsJson1_1 alone. RPC protocols do not stream blobs, so this client leaves `Upload` out.
namespace smithy.rust.codegen.test.multiprotocol

use aws.protocols#awsJson1_1

@awsJson1_1
service MultiProtocolService {
    version: "2024-01-01"
    operations: [
        Greet
        Ping
        NoInputOutput
        Subscribe
        Publish
        StructurePayload
        UnionPayload
        ResponseBindings
        RichData
        ModeledDefaults
        NullableValues
        DocumentValues
    ]
}
