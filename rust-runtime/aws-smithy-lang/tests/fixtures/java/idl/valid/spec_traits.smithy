$version: "2"
namespace smithy.example

@length(min: 1, max: 100)
@documentation("Contains a string")
string MyString

@documentation("An animal in the animal kingdom")
structure Animal {
    @required
    name: smithy.api#String,

    @length(min: 0)
    @tags(["private-beta", "metered"])
    age: smithy.api#String,
}

@trait
structure structuredTrait {
    bar: String
    qux: String
}

@structuredTrait(bar: "baz", qux: "true")
string Sugared

@structuredTrait({bar: "baz", qux: "true"})
string Explicit
