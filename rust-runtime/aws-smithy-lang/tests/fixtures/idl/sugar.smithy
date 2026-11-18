$version: "2"

metadata greeting = "hello"
metadata "quoted key" = {
    nested: [1, -2.5, true, null, "text", required]
    big: 18446744073709551615
}

namespace smithy.example

use other#Imported

/// Documentation for `Thing`.
///
/// Second paragraph.
@tags
@length(min: 1, max: 10)
string Thing

@trait
structure marker {
    level: Integer
}

@marker
structure Example {
    /// Member docs.
    @required
    name: Thing = "a"

    count: Integer = 0
    imported: Imported
    text: String = """
        Hello,
          text block.
        """
}

@marker(level: 2)
list Things {
    member: Thing
}

map Index {
    key: String
    value: Things
}

enum Suit {
    DIAMOND = "diamond"

    @deprecated
    CLUB
}

intEnum Level {
    LOW = 1
    HIGH = 2
}

union Choice {
    a: String
    b: Example
}

timestamp When

document Anything

service Service {
    version: "2024-01-01"
    operations: [GetThing]
    resources: [Widget]
    errors: [Failure]
    rename: { "other#Imported": "ImportedBlob" }
}

resource Widget {
    identifiers: { widgetId: String }
    properties: { label: String }
    read: GetWidget
    operations: [Touch]
}

@readonly
operation GetWidget {
    input: GetWidgetRequest
    output: GetWidgetResponse
    errors: [Failure]
}

structure GetWidgetRequest for Widget {
    @required
    $widgetId
}

structure GetWidgetResponse {
    label: String
}

operation Touch {}

@readonly
operation GetThing {
    input := {
        id: String
    }
    output := @sensitive {
        thing: Thing
    }
}

@error("client")
structure Failure {}
