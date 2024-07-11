$version: "2"

namespace smithy.rust

@documentation(
    "Indicates a shape should support Rust's serde library.
  When a structure or union is marked with this trait, the generator in this package will auto-generate
  implementations of the serde::ser::Serialize and serde::de::Deserialize traits."
)
@trait(selector: ":is(structure, union, enum, string, map, service, operation)")
@internal
structure serde {
    @documentation("Generate support for serde::ser::Serialize")
    serialize: Boolean = true

    @documentation("Generate support for serde::de::Deserialize")
    deserialize: Boolean = true
}
