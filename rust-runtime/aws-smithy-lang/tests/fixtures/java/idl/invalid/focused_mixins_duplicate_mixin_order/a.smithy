$version: "2"
namespace ex

@mixin
@documentation("1")
structure M1 {}

@mixin
@documentation("2")
structure M2 {}

structure A with [M1, M2] {}
