$version: "2"
namespace ex

// The reference implementation drops A, and B, which uses it, without a diagnostic.
@mixin
structure A with [A] {
    x: String
}

structure B with [A] {}

structure C {
    s: String
}
