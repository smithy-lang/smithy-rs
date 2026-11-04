$version: "2"

// Binds the shared compatibility operations to single- and combined-protocol service variants.
// The integration suites compare their routing, payload serialization, response bindings,
// extractor failures, and document handling using common handlers and request scenarios.

// Services under test: the same operations served by different protocol sets.
namespace com.example.review

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restJson1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor

@restJson1
service ReviewRest {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@restJson1
@rpcv2Cbor
service ReviewRestCbor {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@restJson1
@awsJson1_0
service ReviewRestAwsJson {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@restJson1
@restXml
service ReviewRestXml {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@awsJson1_0
service ReviewAwsJson {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@rpcv2Cbor
service ReviewCbor {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@rpcv2Cbor
@awsJson1_0
@awsJson1_1
@restJson1
@restXml
service ReviewAll {
    version: "2024-01-01"
    operations: [
        GetWidget
        DeleteWidget
        Ping
        ResponseCode
        ResponseCodeMember
        ResponseHeader
        StructPayload
        DownloadBlob
        PrefixHeaders
        MediaTypeMap
        TrailingSlashLabel
        GreedyLabel
        SensitiveAmount
        SensitiveTimestamp
    ]
}

@aws.protocols#restJson1
service ReviewDocument {
    version: "2024-01-01"
    operations: [NullDocument]
}
