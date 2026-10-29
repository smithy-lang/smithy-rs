$version: "2"

// Defines string and enum prefix-header inputs for REST JSON and REST XML. Echo handlers
// expose parsed map values so tests compare surrounding-whitespace handling, empty values,
// and invalid enum rejection against legacy behavior.

namespace compat.prefixheaders

use aws.protocols#restJson1
use aws.protocols#restXml

@restJson1
service JsonPrefixHeaders { version: "1", operations: [Echo, EchoEnum] }

@restXml
service XmlPrefixHeaders { version: "1", operations: [Echo, EchoEnum] }

// HttpHeaderValueParserGenerator uses one_or_none_bytes for plain strings,
// including map values bound through HttpBindingGenerator's prefix parser.
@http(method: "GET", uri: "/echo")
operation Echo {
    input := {
        @httpPrefixHeaders("X-Meta-") meta: StringMap
        @httpHeader("x-direct") direct: String
        @httpQuery("q") q: String
        @httpQueryParams params: StringMap
    }
    output := { @httpPayload value: String }
}
map StringMap { key: String, value: String }

@http(method: "GET", uri: "/enum")
operation EchoEnum {
    input := { @httpPrefixHeaders("x-color-") meta: ColorMap }
    output := { @httpPayload value: String }
}
enum Color {
    RED = "red"
    BLUE = "blue"
}
map ColorMap { key: String, value: Color }
