$version: "2"
namespace test
use smithy.protocols#rpcv2Cbor
@rpcv2Cbor
service Example { version: "1" operations: [Write] }
@http(method: "PUT", uri: "/things/{name}", code: 201)
operation Write {
    input := { @required @httpLabel name: String }
    output: Out
    errors: [ClientFailure, ServerFailure, DefaultClientFailure, DefaultServerFailure]
}
structure Out {
    name: String
    @httpHeader("ETag") etag: String
    @httpPrefixHeaders("x-meta-") headers: StringMap
    @httpResponseCode code: Integer
}
map StringMap { key: String value: String }
@error("client")
@httpError(422)
structure ClientFailure {
    @httpHeader("ETag") etag: String
    message: String
    @httpPrefixHeaders("x-meta-") headers: StringMap
}
@error("server")
@httpError(503)
structure ServerFailure {
    message: String
    @httpHeader("ETag") etag: String
    @httpPrefixHeaders("x-meta-") headers: StringMap
}
@error("client")
structure DefaultClientFailure {
    message: String
    @httpHeader("ETag") etag: String
    @httpPrefixHeaders("x-meta-") headers: StringMap
}
@error("server")
structure DefaultServerFailure {
    message: String
    @httpHeader("ETag") etag: String
    @httpPrefixHeaders("x-meta-") headers: StringMap
}
