$version: "2"

namespace ex

structure S {
    m: String
}

apply Missing @documentation("x")

apply S$missing @required
