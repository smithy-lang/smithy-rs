$version: "2"
namespace ex

@mixin
structure M {
    m: String
}

@mixin
structure N {
    n: String
}

// Like the reference implementation, the repeated `M` has no effect: `with [M, N]`.
structure S with [M, N, M] {
    s: String
}
