$version: "2"
namespace ex

structure U with [Nope] {}

@mixin
structure Uses with [U] {}

structure C with [Uses] {}

structure Fine {}
