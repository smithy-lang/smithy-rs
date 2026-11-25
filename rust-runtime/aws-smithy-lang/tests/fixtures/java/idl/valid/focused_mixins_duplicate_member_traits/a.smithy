$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

// Each shape is defined again in b.smithy. Only one of the two definitions declares `x`, or
// neither does, and the trait on `x` comes from either definition or from an `apply`.
structure Later with [M] {}

structure Kept with [M] {
    @documentation("kept")
    x: String
}

structure AppliedToLater with [M] {}

structure AppliedToKept with [M] {
    x: String
}

structure Elided with [M] {}

apply AppliedToLater$x @documentation("applied")
apply AppliedToKept$x @documentation("applied")
