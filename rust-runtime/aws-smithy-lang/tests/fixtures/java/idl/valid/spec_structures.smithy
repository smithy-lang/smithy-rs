$version: "2"
namespace smithy.example

structure MyStructure {
    foo: String
    baz: Integer
}

/// This is MyStructure.
structure DocumentedStructure {
    /// This is documentation for `foo`.
    @required
    foo: String

    /// This is documentation for `baz`.
    @deprecated
    baz: Integer
}

structure Example {
    normative: Boolean = true
}

structure ExplicitExample {
    @default(true)
    normative: Boolean
}
