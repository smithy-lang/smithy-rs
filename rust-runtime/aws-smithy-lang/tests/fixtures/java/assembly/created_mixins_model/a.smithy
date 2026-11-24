$version: "2"

namespace ex

// Reaches C, which was built in code with M as a mixin.
apply M$x @documentation("later")
apply M @tags(["later"])
apply C$y @tags(["c"])
