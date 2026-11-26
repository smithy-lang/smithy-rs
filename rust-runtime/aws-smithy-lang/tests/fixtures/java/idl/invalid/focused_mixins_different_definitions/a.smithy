$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

@mixin
structure N {}

structure C with [M] {}
