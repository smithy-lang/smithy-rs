$version: "2"

// Defines media-typed prefix-header maps on REST JSON and REST XML inputs. The test corpus
// checks base64 and UTF-8 decoding, scalar cardinality, empty/quoted values, malformed encodings,
// and legacy panic observations versus schema rejection behavior.

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
