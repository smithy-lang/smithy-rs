$version: "2"

// A client of the multi-protocol service speaking rpcv2Cbor alone. RPC protocols do not stream blobs, so this client leaves `Upload` out.
namespace com.example.multiprotocol

use smithy.protocols#rpcv2Cbor

@rpcv2Cbor
service MultiProtocolService {
    version: "2024-01-01"
    operations: [
        Greet
        Ping
        Subscribe
        Publish
    ]
}
