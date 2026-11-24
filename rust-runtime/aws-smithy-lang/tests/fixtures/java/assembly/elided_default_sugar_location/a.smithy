$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

structure S with [M] {
    $x = "a"
}
