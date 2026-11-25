$version: "2"
namespace ex

structure A with [M] {
    $nope
}

// Only this definition of B declares `$nope`.
structure B with [M] {
    $nope
}
