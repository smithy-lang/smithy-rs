$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

// An applied trait replaces the trait `= value` applies, as it does on a declared member.
structure Applied with [M] {
    $x = "a"
}

apply Applied$x @default("b")

// Defined again in b.smithy with the same default.
structure Repeated with [M] {
    $x = "a"
}

// A written `@default` doesn't merge with the sugar: the applied trait replaces it.
structure Written with [M] {
    @default("b")
    $x = "a"
}

// Defined again in b.smithy without either, which gets the written `@default`.
structure Equal with [M] {
    @default("a")
    $x = "a"
}
