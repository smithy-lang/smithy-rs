$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

structure A with [M] {
    $nope
}

structure B with [M] {}
