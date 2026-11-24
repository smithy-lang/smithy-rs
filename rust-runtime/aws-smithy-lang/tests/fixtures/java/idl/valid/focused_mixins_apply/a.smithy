$version: "2"
namespace ex

@mixin
@tags(["m"])
structure M {
    @documentation("m")
    @tags(["m"])
    x: String
    y: String
}

structure C with [M] {
    s: String
}

apply C$x @tags(["c"])
apply C$x @tags(["c2"])
apply C @tags(["c"])
