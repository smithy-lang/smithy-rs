$version: "2"

// A client of the multi-protocol service speaking awsJson1_0 alone. RPC protocols do not stream blobs, so this client leaves `Upload` out.
namespace com.example.multiprotocol

use aws.protocols#awsJson1_0

@awsJson1_0
service MultiProtocolService {
    version: "2024-01-01"
    operations: [
        Greet
        Ping
        Subscribe
        Publish
    ]
}
