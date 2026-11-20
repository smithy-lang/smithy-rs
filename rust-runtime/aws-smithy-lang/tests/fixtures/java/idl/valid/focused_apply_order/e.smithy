$version: "2"

namespace ex

@tags(["e-inline"])
structure S {
    @tags(["e-inline"])
    m: String
}

apply S {
    @tags(["e-apply"])
}

apply S$m @tags(["e-apply"])
