$version: "2"
namespace ex

resource R {
    identifiers: { id: String }
    properties: { name: Integer }
}

@mixin
structure M {
    name: String
}

structure S for R with [M] {
    $name
}
