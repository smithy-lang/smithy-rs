$version: "2"
namespace ex

@mixin
structure A with [B] {}

@mixin
structure B with [A] {}

// Dropped without a diagnostic, like the reference implementation.
structure C with [A] {
    @required
    x: String
}
