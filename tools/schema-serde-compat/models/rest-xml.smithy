$version: "2"
namespace compat.audit
use aws.protocols#restXml

@restXml
service Audit {
    version: "1"
    operations: [MediaResponse, Modeled, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing,
        NumberLabel, TimestampLabel, XmlBody, StringPayload, BlobPayload,
        XmlPayload, Response, Sensitive]
}

// Source: XmlBindingTraitParserGenerator.parseStructureInner/parseList/parseFlatList/
// mapEntryParser/parseUnion, RestXmlParserGenerator operation root handling.
@http(method: "POST", uri: "/xml")
operation XmlBody {
    input := {
        @xmlAttribute token: String
        @xmlName("renamed") text: String
        integer: Integer
        boolean: Boolean
        timestamp: Timestamp
        list: XmlItems
        @xmlFlattened flat: Strings
        map: XmlEntries
        @xmlFlattened flatMap: StringMap
        nested: XmlNested
        choice: Choice
    }
    output := {}
}
list XmlItems { @xmlName("item") member: String }
map XmlEntries { @xmlName("Key") key: String, @xmlName("Value") value: String }
structure XmlNested { a: String, b: String }

@http(method: "POST", uri: "/xml-payload")
operation XmlPayload { input := { @httpPayload @xmlName("Envelope") value: XmlNested }, output := {} }
