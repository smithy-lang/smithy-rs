$version: "2"
metadata refs = {
    absolute: smithy.api#String
    relative: Integer
    member: smithy.api#Unit
    String: "keys are literal"
}

namespace ex

@trait
structure refs {
    values: StringList
}

list StringList {
    member: String
}

@refs(values: [Target, smithy.api#String, "Quoted"])
string Target
