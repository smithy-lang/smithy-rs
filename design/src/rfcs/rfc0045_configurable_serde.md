<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Implementing `serde::Serialize`
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: client, server

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines how smithy-rs will enable customers to use the [serde](https://serde.rs) library with generated clients & servers. This is a common request
for myriad reasons, but as we have written about [before](https://github.com/awslabs/aws-sdk-rust/issues/269#issuecomment-1227518721) this is a challenging design area. This RFC proposes a new approach: **Rather than implement `Serialize` directly, add a method to types that returns a type that implements `Serialize`.** This solves a number of issues:

  1. It is minimally impactful: It doesn't lock us into one `Serialize` implementation. It contains only one public trait, `SerializeConfigured`. This trait will initially be defined on a per-crate basis to avoid the orphan-trait rule. It also doesn't have any impact on shared runtime crates (since no types actually need to implement serialize).
  2. It allows customers to configure serde to their use case. For example, for testing/replay you probably _don't_ want to redact sensitive fields but for logging or other forms of data storage, you may want to redact those fields.
3. The entire implementation is isolated to a single module, making it trivial to feature-gate out.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **`serde`**: A specific [Rust library](https://serde.rs) that is commonly used for serialization
- **`Serializer`**: The serde design decouples the serialization format (e.g. JSON) from the serialization structure of a particular piece of data. This allows the same Rust code to be serialized to CBOR, JSON, etc. The serialization protocol, e.g. `serde_json`, is referred to as the `Serializer`.
- **Decorator**: The interface by which code separated from the core code generator can customize codegen behavior.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------
Currently, there is no practical way for customers to link Smithy-generated types with `Serialize`.
Customers will bring the `SerdeDecorator` into scope by including it on their classpath when generating clients and servers.

Customers may add a `serde` trait to members of their model:
```smithy
use smithy.rust#serde;
@serde
structure SomeStructure {
    field: String
}
```

The `serde` trait can be added to all shapes, including operations and services. When it is applied to a service, all shapes in the service closure will support serialization.

**Note: this RFC only describes `Serialize`. A follow-up RFC and implementation will handle `Deserialize`.**

Generated crates that include at least one `serde` tagged shape will include a `serde` feature. This will feature gate the module containing the serialization logic. This will provide implementations of `SerializeConfigured` which provides two methods:

```rust,ignore
my_thing.serialize_ref(&settings); // Returns `impl Serialize + 'a`
my_thing.serialize_owned(settings); // Returns `impl Serialize`
```

Once a customer has an object that implements `Serialize` they can then use it with any `Serializer` supported by `serde`.
```rust,ignore
use generated_crate::serde::SerializeConfigured;
let my_shape = Shape::builder().field(5).build();
let settings = SerializationSettings::redact_sensitive_fields();
let as_json = serde_json::to_string(&my_shape.serialize_ref(&settings));
```

### Customer Use Cases
#### I want to embed a structure into my own types that implement `Serialize`
The generated code includes two methods:
`serialize_redacted` and `serialize_unredacted`.

> Note: There is nothing in these implementations that rely on implementation details—Customers can implement these methods (or variants of them) themselves.

These have the correct signatures to be used with `serialize_with`:
```rust,ignore
use generated_crate::serde::serialize_redacted;

#[derive(Serialize)]
struct MyStruct {
    #[serde(serialize_with = "serialize_redacted")]
    inner: SayHelloInput,
}
```
#### I want to serialize data for testing
This will be supported in the future. Currently `Deserialize` behavior is not covered by this RFC. Customers can take the same serialization settings they used.


#### I want to dump a structured form of data into a database/logs/etc.
This is possible by using the base APIs. If customers want to delegate another thread or piece of code to actually perform the serialization, they can use `.serialize_owned(..)` along with [erased-serde](https://crates.io/crates/erased-serde) to accomplish this.

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

In order to provide configurable serialization, this defines the crate-local public trait `SerializeConfigured`:
```rust,ignore
/// Trait that allows configuring serialization
/// **This trait should not be implemented directly!** Instead, `impl Serialize for ConfigurableSerdeRef<T>`
pub trait SerializeConfigured {
    /// Return a `Serialize` implementation for this object that owns the object.
    ///
    /// Use this if you need to create `Arc<dyn Serialize>` or similar.
    fn serialize_owned(self, settings: SerializationSettings) -> impl Serialize;

    /// Return a `Serialize` implementation for this object that borrows from the given object
    fn serialize_ref<'a>(&'a self, settings: &'a SerializationSettings) -> impl Serialize + 'a;
}
```

We also need to define `SerializationSettings`. The only setting currently exposed is `redact_sensitive_fields`:
```rust,ignore

#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default)]
pub struct SerializationSettings {
    /// Replace all sensitive fields with `<redacted>` during serialization
    pub redact_sensitive_fields: bool,
}
```

We MAY add additional configuration options in the future, but will keep the default behavior matching current behavior. Future options include:
- Serialize `null` when a field is unset (the current default is to skip serializing that field)
- Serialize blobs via a list of numbers instead of via base64 encoding
- Change the default format for datetimes (current `HttpDate`)

No objects actually implement `SerializeConfigured`. Instead, the crate defines two private structs:
```rust,ignore
pub(crate) struct ConfigurableSerde<T> {
    pub(crate) value: T,
    pub(crate) settings: SerializationSettings
}

pub(crate) struct ConfigurableSerdeRef<'a, T> {
    pub(crate) value: &'a T,
    pub(crate) settings: &'a SerializationSettings
}
```
> **Why two structs?**
>
> We need to support two use cases—one where the customer wants to maintain ownership of their data and another where the customer wants to create `Box<dyn Serialize>` or other fat pointer. There is a blanket impl for `Serialize` from `ConfigurableSerde` to `ConfigurableSerdeRef`.

The `SerializeConfigured` trait has a blanket impl for `ConfigurableSerdeRef`:
```rust,ignore
/// Blanket implementation for all `T` such that `ConfigurableSerdeRef<'a, T>` implements `Serialize`.
impl<T> SerializeConfigured for T
    where for<'a> ConfigurableSerdeRef<'a, T>: Serialize {
    fn serialize_owned(
        self,
        settings: SerializationSettings,
    ) -> impl Serialize {
        ConfigurableSerde {
            value: self,
            settings,
        }
    }

    fn serialize_ref<'a>(
        &'a self,
        settings: &'a SerializationSettings,
    ) -> impl Serialize + 'a {
        ConfigurableSerdeRef {
            value: self,
            settings,
        }
    }
}
```

The job of the code generator is then to implement `ConfigurableSerdeRef` for all the specific `T` that we wish to serialize.

#### Supporting Sensitive Shapes
Handling `@sensitive` is done by wrapping memers in `Sensitive<'a T>(&'a T)` during serialization. The `serialize` implementation consults the settings to determine if redaction is required.

```rust,ignore
if let Some(member_1) = &inner.foo {
    s.serialize_field("foo",
        &Sensitive(&member_1.serialize_ref(&self.settings)).serialize_ref(&self.settings),
    )?;
}
```

Note that the exact mechanism for supporting sensitive shapes is crate-private and can be changed in the future.

#### Supporting Maps and Lists

For Maps and Lists, we need to be able to handle the case where two different `Vec<String>` may be serialized differently. For example, one may target a `Sensitive` string and the other may target a non-sensitive string.

To handle this case, we generate a wrapper struct for collections:

```rust,ignore
struct SomeStructWrapper<'a>(&'a Vec<SomeStruct>);
```

We then implement `Serialize` for this wrapper which allows us to control behavior on a collection-by-collection basis without running into conflicts.

> **Note**: This is a potential area where future optimizations could reduce the amount of generated code if we were able to detect that collection serialization implementations were identical and deduplicate them.

#### Supporting `DateTime`, `Blob`, `Document`, etc.
For custom types that do not implement `Serialize`, we generate crate-private implementations, only when actually needed:
```rust,ignore
impl<'a> Serialize for ConfigurableSerdeRef<'a, DateTime> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        serializer.serialize_str(&self.value.to_string())
    }
}
```


Changes checklist
-----------------

- [x] Define `SerializeConfigured`
- [x] Define `ConfigurableSerde`/`SerdeRef`
- [x] Generate implementations for all types in the service closure
- [x] Handle sensitive shapes
- [ ] Implement `Deserialize`
