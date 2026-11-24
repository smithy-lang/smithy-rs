$version: "2"
namespace ex

@mixin
structure M {
    @documentation("m")
    x: String
    y: String
}

structure C with [M] {
    @required
    x: String
    s: String
}

@mixin
enum EM {
    A
    B = "bee"
}

enum E with [EM] {
    A = "other"
    C
}

@mixin
intEnum IM {
    ONE = 1
}


intEnum IE with [IM] {
    TWO = 2
}

@mixin
union UM {
    a: String
}

union U with [UM] {
    b: Integer
}

@mixin
structure SameA {
    @documentation("A")
    @tags(["a"])
    x: String
}

@mixin
structure SameB {
    @documentation("B")
    x: String
}

structure Same with [SameA, SameB] {}
