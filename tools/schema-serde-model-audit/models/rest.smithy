$version: "2"
namespace audit

@http(method: "GET", uri: "/label/{name}/", code: 200)
operation Label { input := { @required @httpLabel name: String }, output := {} }
@http(method: "GET", uri: "/greedy/{name+}/", code: 200)
operation Greedy { input := { @required @httpLabel name: String }, output := {} }
@http(method: "GET", uri: "/suffix/{name+}/end", code: 200)
operation Suffix { input := { @required @httpLabel name: String }, output := {} }
@http(method: "GET", uri: "/number/{value}", code: 200)
operation NumberLabel { input := { @required @httpLabel value: Integer }, output := {} }
@http(method: "GET", uri: "/time/{value}", code: 200)
operation TimeLabel { input := { @required @httpLabel value: Timestamp }, output := {} }
@http(method: "GET", uri: "/query?fixed=yes&flag", code: 200)
operation Query { input: QueryValues, output := {} }
structure QueryValues {
    @httpQuery("text") text: String
    @httpQuery("integer") integer: Integer
    @httpQuery("boolean") boolean: Boolean
    @httpQuery("list") list: Strings
    @httpQuery("number") numbers: Numbers
    @httpQuery("time") time: Timestamp
    @httpQueryParams extras: MultiEntries
}
@http(method: "GET", uri: "/query-map", code: 200)
operation QueryMap { input := { @httpQueryParams extras: Entries }, output := {} }

@http(method: "POST", uri: "/headers", code: 200)
operation Headers { input: HeaderValues, output: HeaderValues }
structure HeaderValues {
    @httpHeader("x-text") text: String
    @httpHeader("x-list") list: Strings
    @httpHeader("x-integer") integer: Integer
    @httpHeader("x-boolean") boolean: Boolean
    @httpHeader("x-date") date: Timestamp
    @httpHeader("x-dates") dates: Dates
    @httpHeader("x-media") media: Media
    @httpHeader("x-meta-explicit") explicit: String
    @httpPrefixHeaders("X-Meta-") extras: MultiEntries
}
list Dates { member: Timestamp }
@mediaType("text/plain")
string Media
@http(method: "POST", uri: "/prefix-all", code: 200)
operation PrefixAll { input := { @httpPrefixHeaders("") extras: Entries }, output := {} }

@http(method: "POST", uri: "/text", code: 200)
operation TextPayload { input: TextData, output: TextData }
structure TextData { @httpPayload body: String }
@http(method: "POST", uri: "/blob", code: 200)
operation BlobPayload { input: BlobData, output: BlobData }
structure BlobData { @httpPayload body: Blob }
@http(method: "POST", uri: "/struct", code: 200)
operation StructPayload { input: StructData, output: StructData }
structure StructData { @httpPayload body: Nested }
@http(method: "POST", uri: "/union", code: 200)
operation UnionPayload { input: UnionData, output: UnionData }
structure UnionData { @httpPayload body: Choice }
