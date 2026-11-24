$version: "2"
namespace ex

use other#M

structure C with [M] {}

// `@mixin` applied after the shape that uses it.
structure P {
    x: String
}

structure UsesP with [P] {}

apply P @mixin
