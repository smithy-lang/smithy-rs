$version: "2"
namespace audit

// Each member selects a different legacy parser/serializer branch. Reuse exactly
// this model in both modes; request bytes live in cases.py.
@http(method: "POST", uri: "/echo", code: 200)
operation Echo { input: Values, output: Values, errors: [Failure] }
structure Values {
    @jsonName("wireText") text: String
    boolean: Boolean
    byte: Byte
    short: Short
    integer: Integer
    long: Long
    float: Float
    double: Double
    blob: Blob
    time: Timestamp
    @timestampFormat("date-time") date: Timestamp
    @timestampFormat("http-date") httpDate: Timestamp
    count: Integer = 7
    strings: Strings
    numbers: Numbers
    entries: Entries
    nested: Nested
    choice: Choice
    color: Color
    fail: Boolean
}
structure Nested { value: String, next: Nested }
union Choice { text: String, number: Integer, nested: Nested, items: Strings, unit: Unit }
enum Color { RED = "red", BLUE = "blue" }
list Strings { member: String }
list Numbers { member: Integer }
map Entries { key: String, value: String }
map MultiEntries { key: String, value: Strings }
@error("client") @httpError(409)
structure Failure { message: String, @httpHeader("x-error") header: String }

@http(method: "POST", uri: "/empty", code: 200)
operation Empty { input := {}, output := {} }
@http(method: "POST", uri: "/unit", code: 204)
operation NoInput {}

@http(method: "POST", uri: "/sparse", code: 200)
operation Sparse { input: SparseValues, output: SparseValues }
structure SparseValues { strings: SparseStrings, entries: SparseEntries }
@sparse
list SparseStrings { member: String }
@sparse
map SparseEntries { key: String, value: String }

@http(method: "POST", uri: "/document", code: 200)
operation DocumentValue { input: Documents, output: Documents }
structure Documents { value: Document }
