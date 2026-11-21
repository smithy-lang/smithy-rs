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
