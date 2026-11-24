$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

operation O {
    input: M
}

structure S {
    m: M
}
