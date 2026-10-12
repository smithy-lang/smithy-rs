$version: "2"

// The server side: one service declaring every built-in protocol.
namespace com.example.multiprotocol

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restJson1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor

@rpcv2Cbor
@awsJson1_0
@awsJson1_1
@restJson1
@restXml
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
