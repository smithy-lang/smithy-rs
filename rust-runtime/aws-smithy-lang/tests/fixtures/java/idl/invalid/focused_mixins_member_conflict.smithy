$version: "2"
namespace ex

@mixin
structure A {
    a: String
}

@mixin
structure B {
    a: Integer
}

@mixin
structure U with [A, B] {}

structure C with [U] {}
