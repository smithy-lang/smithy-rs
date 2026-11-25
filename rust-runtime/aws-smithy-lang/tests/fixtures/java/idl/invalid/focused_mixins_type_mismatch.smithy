$version: "2"
namespace ex

@mixin
union M {
    a: String
}

structure S with [M] {}

structure C with [S] {}
