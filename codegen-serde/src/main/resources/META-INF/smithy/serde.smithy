$version: "2"

namespace smithy.rust

@documentation(
    "Indicates a shape should support Rust's [serde](https://serde.rs/) library.
  When a shape is marked with this trait, the generator in this package will auto-generate
  implementations of the `serde::ser::Serialize` trait. Support for `Deserialize` is currently
   unsupported. When applied to a service, all shapes in the service closure will implement these traits."
)
@trait(selector: ":is(structure, union, enum, string, map, service, operation)")
@internal
structure serde {
    @documentation("Generate support for serde::ser::Serialize")
    serialize: Boolean = true

    @documentation("Generate support for serde::ser::Deserialize. This is not currently supported.")
    deserialize: Boolean = false
}
