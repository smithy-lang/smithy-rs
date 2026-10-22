$version: "2"
namespace ex

@trait
structure structTrait {}

@trait
list listTrait {
    member: String
}

@trait
map mapTrait {
    key: String
    value: String
}

@trait
union unionTrait {
    a: String
}

@trait
document documentTrait

@structTrait
@listTrait
@mapTrait
@documentTrait
string Omitted

@structTrait()
@listTrait()
@mapTrait()
@documentTrait()
string EmptyParens

@unionTrait(a: "x")
@documentTrait(key: "value")
string StructuredSyntax
