$version: "2"

namespace smithy.rust

@documentation(
    "Indicates a shape should support Rust's [serde](https://serde.rs/) library.
  When a shape is marked with this trait, the generator in this package will auto-generate
  configurable serialization and deserialization support. This support is provided for convenience
  only. It is not used for Smithy protocol wire serialization or deserialization, and its
  representations are not guaranteed to match any protocol wire format. When applied to a service,
  all supported shapes in the service closure will support the enabled directions."
)
@trait(selector: ":is(structure, union, enum, string, map, service, operation)")
@internal
structure serde {
    @documentation("Generate support for serde::ser::Serialize")
    serialize: Boolean = true

    @documentation("Generate support for serde deserialization.")
    deserialize: Boolean = false
}
