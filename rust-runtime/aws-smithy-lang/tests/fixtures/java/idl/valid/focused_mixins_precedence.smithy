$version: "2"
namespace ex

@mixin
@documentation("A")
@tags(["a"])
structure A {
    a: String
    @documentation("A shared")
    shared: String
}

@mixin
@documentation("B")
structure B {
    @documentation("B shared")
    @required
    shared: String
    b: Integer
}

/// The shape's own documentation wins.
structure D with [A, B] {
    s: String
}

structure E with [A, B] {}

@mixin
structure Base {
    base: String
}

@mixin
structure Mid with [Base] {
    mid: String
}

@mixin
structure Other {
    other: String
}

structure Leaf with [Mid, Other] {
    leaf: String
}
