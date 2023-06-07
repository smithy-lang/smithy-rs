# Aggregate Shapes

| Smithy Type | Rust Type |
| ----------- | ----------- |
| [List](#list) | `Vec<Member>` |
| [Set](#set) | `Vec<Member>` |
| [Map](#map) | `HashMap<String, Value>` |
| [Structure](#structure) | `struct` |
| [Union](#union) | `enum` |

Most generated types are controlled by [SymbolVisitor](https://github.com/awslabs/smithy-rs/blob/main/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/SymbolVisitor.kt).

## List
List objects in Smithy are transformed into vectors in Rust. Based on the output of the [NullableIndex](https://awslabs.github.io/smithy/javadoc/1.5.1/software/amazon/smithy/model/knowledge/NullableIndex.html), the generated list may be `Vec<T>` or `Vec<Option<T>>`.

## Set
Because floats are not Hashable in Rust, for simplicity smithy-rs translates all sets to into `Vec<T>` instead of `HashSet<T>`. In the future, a breaking change may be made to introduce a library-provided wrapper type for Sets.

## Map
Because `key` MUST be a string in Smithy maps, we avoid the hashibility issue encountered with `Set`. There are optimizations that could be considered (e.g. since these maps will probably never be modified), however, pending customer feedback, Smithy Maps become `HashMap<String, V>` in Rust.

## Structure
> See `StructureGenerator.kt` for more details

Smithy `structure` becomes a `struct` in Rust. Backwards compatibility & usability concerns lead to a few design choices:

  1. As specified by `NullableIndex`, fields are `Option<T>` when Smithy models them as nullable.
  2. All structs are marked `#[non_exhaustive]`
  3. All structs derive `Debug` & `PartialEq`. Structs **do not** derive `Eq` because a `float` member may be added in the future.
  4. Struct fields are public. Public struct fields allow for [split borrows](https://doc.rust-lang.org/nomicon/borrow-splitting.html). When working with output objects this significantly improves ergonomics, especially with optional fields.
      ```rust,ignore
      let out = dynamo::ListTablesOutput::new();
      out.some_field.unwrap(); // <- partial move, impossible with an accessor
      ```
  5. Builders are generated for structs that provide ergonomic and backwards compatible constructors. A builder for a struct is always available via the convenience method `SomeStruct::builder()`
  6. Structures manually implement debug: In order to support the [sensitive trait](https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html#sensitive-trait), a `Debug` implementation for structures is manually generated.

### Example Structure Output
**Smithy Input**:

```java
@documentation("<p>Contains I/O usage metrics...")
structure IOUsage {
    @documentation("... elided")
    ReadIOs: ReadIOs,
    @documentation("... elided")
    WriteIOs: WriteIOs
}

long ReadIOs

long WriteIOs
```
**Rust Output**:
```rust,ignore
/// <p>Contains I/O usage metrics for a command that was invoked.</p>
#[non_exhaustive]
#[derive(std::clone::Clone, std::cmp::PartialEq)]
pub struct IoUsage {
    /// <p>The number of read I/O requests that the command made.</p>
    pub read_i_os: i64,
    /// <p>The number of write I/O requests that the command made.</p>
    pub write_i_os: i64,
}
impl std::fmt::Debug for IoUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut formatter = f.debug_struct("IoUsage");
        formatter.field("read_i_os", &self.read_i_os);
        formatter.field("write_i_os", &self.write_i_os);
        formatter.finish()
    }
}
/// See [`IoUsage`](crate::model::IoUsage)
pub mod io_usage {
    /// A builder for [`IoUsage`](crate::model::IoUsage)
    #[non_exhaustive]
    #[derive(Debug, Clone, Default)]
    pub struct Builder {
        read_i_os: std::option::Option<i64>,
        write_i_os: std::option::Option<i64>,
    }
    impl Builder {
        /// <p>The number of read I/O requests that the command made.</p>
        pub fn read_i_os(mut self, inp: i64) -> Self {
            self.read_i_os = Some(inp);
            self
        }
         /// <p>The number of read I/O requests that the command made.</p>
        pub fn set_read_i_os(mut self, inp: Option<i64>) -> Self {
            self.read_i_os = inp;
            self
        }
        /// <p>The number of write I/O requests that the command made.</p>
        pub fn write_i_os(mut self, inp: i64) -> Self {
            self.write_i_os = Some(inp);
            self
        }
        /// <p>The number of write I/O requests that the command made.</p>
        pub fn set_write_i_os(mut self, inp: Option<i64>) -> Self {
            self.write_i_os = inp;
            self
        }
        /// Consumes the builder and constructs a [`IoUsage`](crate::model::IoUsage)
        pub fn build(self) -> crate::model::IoUsage {
            crate::model::IoUsage {
                read_i_os: self.read_i_os.unwrap_or_default(),
                write_i_os: self.write_i_os.unwrap_or_default(),
            }
        }
    }
}
impl IoUsage {
    /// Creates a new builder-style object to manufacture [`IoUsage`](crate::model::IoUsage)
    pub fn builder() -> crate::model::io_usage::Builder {
        crate::model::io_usage::Builder::default()
    }
}
```

## Union
Smithy `Union` is modeled as `enum` in Rust.

1. Generated `enum`s must be marked `#[non_exhaustive]`.
2. Generated `enum`s must provide an `Unknown` variant. If parsing receives an unknown input that doesn't match any of the given union variants, `Unknown` should be constructed. [Tracking Issue](https://github.com/awslabs/smithy-rs/issues/185).
3. Union members (enum variants) are **not** nullable, because Smithy union members cannot contain null values.
4. When union members contain references to other shapes, we generate a wrapping variant (see below).
5. Union members do not require `#[non_exhaustive]`, because changing the shape targeted by a union member is not backwards compatible.
6. `is_variant` and `as_variant` helper functions are generated to improve ergonomics.

### Generated Union Example
The union generated for a simplified `dynamodb::AttributeValue`
**Smithy**:
```java
namespace test

union AttributeValue {
    @documentation("A string value")
    string: String,
    bool: Boolean,
    bools: BoolList,
    map: ValueMap
}

map ValueMap {
    key: String,
    value: AttributeValue
}

list BoolList {
    member: Boolean
}
```
**Rust**:
```rust,ignore
#[non_exhaustive]
#[derive(std::clone::Clone, std::cmp::PartialEq, std::fmt::Debug)]
pub enum AttributeValue {
    /// a string value
    String(std::string::String),
    Bool(bool),
    Bools(std::vec::Vec<bool>),
    Map(std::collections::HashMap<std::string::String, crate::model::AttributeValue>),
}

impl AttributeValue {
    pub fn as_bool(&self) -> Result<&bool, &crate::model::AttributeValue> {
        if let AttributeValue::Bool(val) = &self { Ok(&val) } else { Err(self) }
    }
    pub fn is_bool(&self) -> bool {
        self.as_bool().is_some()
    }
    pub fn as_bools(&self) -> Result<&std::vec::Vec<bool>, &crate::model::AttributeValue> {
        if let AttributeValue::Bools(val) = &self { Ok(&val) } else { Err(self) }
    }
    pub fn is_bools(&self) -> bool {
        self.as_bools().is_some()
    }
    pub fn as_map(&self) -> Result<&std::collections::HashMap<std::string::String, crate::model::AttributeValue>, &crate::model::AttributeValue> {
        if let AttributeValue::Map(val) = &self { Ok(&val) } else { Err(self) }
    }
    pub fn is_map(&self) -> bool {
        self.as_map().is_some()
    }
    pub fn as_string(&self) -> Result<&std::string::String, &crate::model::AttributeValue> {
        if let AttributeValue::String(val) = &self { Ok(&val) } else { Err(self) }
    }
    pub fn is_string(&self) -> bool {
        self.as_string().is_some()
    }
}
```
