$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

// An applied trait replaces a value assignment, as it does on a declared member.
structure Applied with [M] {
    $x = "a"
}

apply Applied$x @default("b")

// Defined again in b.smithy with the same assignment.
structure Repeated with [M] {
    $x = "a"
}
