$version: "2"
namespace ex

@mixin
structure M {
    x: String
}

// Only this definition has a default, so the two definitions differ.
structure S with [M] {
    $x = "a"
}
