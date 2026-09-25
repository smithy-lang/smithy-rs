$version: "2"

// A client of the multi-protocol service speaking restJson1 alone. REST protocols stream blobs, so this client calls `Upload` too.
namespace com.example.multiprotocol

use aws.protocols#restJson1

@restJson1
service MultiProtocolService {
    version: "2024-01-01"
    operations: [
        Greet
        Ping
        Upload
        Subscribe
        Publish
    ]
}
