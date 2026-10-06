// coverage-service: test.defaults#DefaultsService
// coverage-plugins: server
$version: "2.0"

namespace test.defaults

use aws.protocols#restJson1

@restJson1
service DefaultsService {
    version: "1.0"
    operations: [PutDefaults]
}

@http(method: "POST", uri: "/defaults")
operation PutDefaults {
    input := {
        @default("2021-07-08T09:00:00Z")
        isoTimestamp: Timestamp

        @default(1625734800)
        epochTimestamp: Timestamp

        @default([])
        emptyList: StringList

        @default({})
        emptyMap: StringMap

        @default(null)
        nullDoc: Document

        @default(true)
        boolDoc: Document

        @default("hello")
        stringDoc: Document

        @default(7)
        posIntDoc: Document

        @default(-7)
        negIntDoc: Document

        @default(1.5)
        floatDoc: Document

        @default("hi")
        defaultString: String

        @default(false)
        defaultBool: Boolean

        @default(9)
        defaultInt: Integer

        @default(1.25)
        defaultDouble: Double

        @default("QUJD")
        defaultBlob: Blob

        @default("red")
        defaultEnum: Color

        @default(2)
        defaultIntEnum: Level
    }
    output := {
        ok: Boolean
    }
}

list StringList {
    member: String
}

map StringMap {
    key: String
    value: String
}

enum Color {
    RED = "red"
    BLUE = "blue"
}

intEnum Level {
    ONE = 1
    TWO = 2
}
