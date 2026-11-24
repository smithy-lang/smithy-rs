$version: "2"
namespace ex

structure Later with [M] {
    @documentation("later")
    x: String
}

structure Kept with [M] {}

structure AppliedToLater with [M] {
    x: String
}

structure AppliedToKept with [M] {}

structure Elided with [M] {
    @documentation("elided")
    $x
}
