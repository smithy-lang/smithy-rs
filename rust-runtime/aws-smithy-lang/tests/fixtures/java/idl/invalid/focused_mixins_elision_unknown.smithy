$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

structure C with [M] {
    @required
    $nope
}
