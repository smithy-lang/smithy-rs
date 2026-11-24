$version: "2"
namespace ex

@mixin
structure A {
    x: String
}

@mixin
structure B {
    x: Integer
}

// The applied trait gives C$x its own traits, so C is built and only the member conflict is
// reported.
structure C with [A, B] {}

apply C$x @required
