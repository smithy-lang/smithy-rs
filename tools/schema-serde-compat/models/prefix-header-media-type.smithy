$version: "2"
namespace compat.prefixmedia

use aws.protocols#restJson1
use aws.protocols#restXml

@restJson1
service JsonPrefixMedia { version: "1", operations: [Echo] }

@restXml
service XmlPrefixMedia { version: "1", operations: [Echo] }

// HttpHeaderValueParserGenerator tokenizes media-typed header values, decodes
// base64, validates UTF-8, then checks scalar cardinality. The prefix parser
// expects a value for every matching header name, including empty headers.
@http(method: "GET", uri: "/echo")
operation Echo {
    input := {
        @httpPrefixHeaders("X-Media-") meta: MediaMap
        @httpHeader("x-direct") direct: MediaString
        @httpQuery("q") q: MediaString
        @httpQueryParams params: MediaMap
    }
    output := { @httpPayload value: String }
}

@mediaType("application/json")
string MediaString
map MediaMap { key: String, value: MediaString }
