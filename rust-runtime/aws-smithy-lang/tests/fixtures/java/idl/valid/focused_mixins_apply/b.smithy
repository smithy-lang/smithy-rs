$version: "2"
namespace ex

// Traits applied to a mixin and its members reach the shapes that use it.
apply M$y @documentation("later")
apply M$x @tags(["m2"])
apply M @documentation("mixin doc")
