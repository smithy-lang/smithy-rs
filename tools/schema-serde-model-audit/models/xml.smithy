$version: "2"
namespace audit

@http(method: "POST", uri: "/xml", code: 200)
operation Xml { input: XmlValues, output: XmlValues }
@xmlName("Envelope")
@xmlNamespace(uri: "urn:audit")
structure XmlValues {
    @xmlAttribute @xmlName("id") identifier: String
    @xmlName("renamed") text: String
    wrapped: NamedStrings
    @xmlFlattened flat: NamedStrings
    mapping: NamedEntries
    @xmlFlattened flatMap: NamedEntries
    choice: XmlChoice
}
list NamedStrings { @xmlName("item") member: String }
map NamedEntries { @xmlName("K") key: String, @xmlName("V") value: String }
union XmlChoice { @xmlName("words") @xmlFlattened items: NamedStrings, text: String, unit: Unit }
