$version: "2"
namespace ex

@mixin
structure M {
    @required
    id: String
    name: String
    other: Integer
}

resource R {
    identifiers: { id: String }
    properties: { name: String, size: Integer }
    read: GetR
}

@readonly
operation GetR {
    input := for R {
        @required
        $id
    }
    output := for R {
        $name
        $size
    }
}

// Identifiers, then properties, then mixin members.
structure S for R with [M] {
    $id
    $name
    $other
    $size
}

structure C with [M] {
    @documentation("c")
    $id
}

operation O {
    input := with [M] {
        $id
        extra: String
    }
}
