$version: "2"

// An rpcv2Cbor error with a header-bound member. rpcv2Cbor ignores HTTP bindings, so the member
// belongs in the document; the generated error serializer must still compile.
namespace com.example.review

use smithy.protocols#rpcv2Cbor

@rpcv2Cbor
service ReviewCborErrors {
    version: "2024-01-01"
    operations: [FailWithHeader]
}

operation FailWithHeader {
    input := {}
    output := {}
    errors: [HeaderedFailure]
}

@error("client")
structure HeaderedFailure {
    message: String

    @httpHeader("x-detail")
    detail: String
}
