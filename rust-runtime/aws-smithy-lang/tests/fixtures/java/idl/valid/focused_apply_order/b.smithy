$version: "2"

namespace ex

apply S @tags(["b-apply"])

@tags(["b-inline"])
structure S {
    @tags(["b-inline"])
    m: String
}
