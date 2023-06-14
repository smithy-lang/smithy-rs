# Simple Shapes
| Smithy Type (links to design discussions) | Rust Type (links to Rust documentation)   |
| ----------- | ----------- |
| blob | `Vec<u8>` |
| boolean | [`bool`](https://doc.rust-lang.org/std/primitive.bool.html) |
| [string](#strings)  | [`String`](https://doc.rust-lang.org/std/string/struct.String.html) |
| byte   | `i8` |
| short  | `i16` |
| integer | `i32` |
| long | `i64` |
| float | `f32` |
| double | `f64` |
| [bigInteger](#big-numbers) | `BigInteger` (Not implemented yet) |
| [bigDecimal](#big-numbers) | `BigDecimal` (Not implemented yet) |
| [timestamp](#timestamps)  | [`DateTime`](https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-types/src/date_time/mod.rs) |
| [document](#documents) | [`Document`](https://github.com/awslabs/smithy-rs/blob/v0.14/rust-runtime/aws-smithy-types/src/lib.rs#L38-L52) |

### Big Numbers
Rust currently has no standard library or universally accepted large-number crate. Until one is stabilized, a string representation is a reasonable compromise:

```rust
pub struct BigInteger(String);
pub struct BigDecimal(String);
```

This will enable us to add helpers over time as requested. Users will also be able to define their own conversions into their preferred large-number libraries.

As of 5/23/2021 BigInteger / BigDecimal are not included in AWS models. Implementation is tracked [here](https://github.com/awslabs/smithy-rs/issues/312).
### Timestamps
[chrono](https://github.com/chronotope/chrono) is the current de facto library for datetime in Rust, but it is pre-1.0. DateTimes are represented by an SDK defined structure modeled on `std::time::Duration` from the Rust standard library.

```rust
{{#include ../../../rust-runtime/aws-smithy-types/src/date_time/mod.rs:date_time}}
```

Functions in the `aws-smithy-types-convert` crate provide conversions to other crates, such as `time` or `chrono`.

### Strings
Rust has two different String representations:
* `String`, an owned, heap allocated string.
* `&str`, a reference to a string, owned elsewhere.

In ideal world, input shapes, where there is no reason for the strings to be owned would use `&'a str`. Outputs would likely use `String`. However, Smithy does not provide a distinction between input and output shapes.

A third compromise could be storing `Arc<String>`, an atomic reference counted pointer to a `String`. This may be ideal for certain advanced users, but is likely to confuse most users and produces worse ergonomics. _This is an open design area where we will seek user feedback._ Rusoto uses `String` and there has been [one feature request](https://github.com/rusoto/rusoto/issues/1806) to date to change that.

Current models represent strings as `String`.

### Document Types

Smithy defines the concept of "Document Types":
> [Documents represent] protocol-agnostic open content that is accessed like JSON data. Open content is useful for modeling unstructured data that has no schema, data that can't be modeled using rigid types, or data that has a schema that evolves outside of the purview of a model. The serialization format of a document is an implementation detail of a protocol and MUST NOT have any effect on the types exposed by tooling to represent a document value.

```rust,ignore
{{#include ../../../rust-runtime/aws-smithy-types/src/lib.rs:document}}
```

Individual protocols define their own document serialization behavior, with some protocols such as AWS and EC2 Query not supporting document types.
