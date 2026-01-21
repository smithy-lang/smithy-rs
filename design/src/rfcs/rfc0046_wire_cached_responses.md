RFC: Wire-Cached Responses
==========================

> Status: RFC
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC defines a mechanism for caching wire-format responses in smithy-rs services using CBOR protocol, enabling efficient storage and retrieval of serialized response data without full deserialization and reserialization.

Terminology
-----------

- **Wire-cached response**: A response that allows the wire format to be used directly
- **@cacheable trait**: A smithy-rs specific trait that can be applied to structure members to enable wire caching
- **CBOR**: Concise Binary Object Representation, a binary data serialization format
- **Cacheable enum**: A Rust enum that can hold either modeled data OR cached bytes

The user experience if this RFC is implemented
----------------------------------------------

Today, if a smithy-rs server wants to cache response data, they must store it in some way (which is not publicly exposed), then during deserialization, they need to deserialize it from their cache, only to _reserialize_ it again. This is inefficient.

Once this RFC is implemented, server developers will be able to:

1. **Mark fields as cacheable** by applying the `@cacheable` trait to structure members in their Smithy models:

```smithy
structure GetUserResponse {
    @cacheable
    userData: UserData,

    // Non-cacheable fields work as before
    requestId: String
}
```

2. **Store both parsed or serialized data** through the generated `Cacheable<T>` enum in server handlers:

```rust
// Generated code will look like this:
pub struct GetUserResponse {
    pub user_data: Cacheable<UserData>,
    pub request_id: Option<String>,
}

// Usage in server handler implementation:
async fn get_user(input: GetUserInput) -> Result<GetUserOutput, GetUserError> {
    // Server can choose to work with parsed data or preserve wire format
    let user_data = match cached_user_data {
        Some(bytes) => Cacheable::Cached(bytes), // Use cached wire format
        None => {
            let parsed = fetch_user_data(&input.user_id).await?;
            let serialized_form = parsed.to_bytes();
            cache_user_data(compute_cache_key(&input), serialized_form.clone()).await?; // serialized_form is `Bytes` which is cheap-to-clone
            // cache the data, then return the cached value:
            Cacheable::Cached(serialized_form)
        }
    };

    Ok(GetUserOutput { user_data, ..Default::default() })
}
```

3. **Extract cached bytes** using the generated `to_bytes()` method on the target shape:

```rust
// The to_bytes() method is generated on the UserData struct itself
let user_data = UserData { name: "Alice".to_string(), age: 30 };
let cached_bytes = user_data.to_bytes();
database.store_cached_response("user_123", cached_bytes);
```

This enables efficient caching scenarios where server implementations can store the original wire format and avoid re-serialization costs when serving cached responses.

**Important:** Providing cached data puts a responsibility on the caller to ensure that the cached data is valid for the given context. `smithy-rs` does NOT validate this by default. Callers are responsible for maintaining data integrity.


### Backwards compatibility

This feature is entirely opt-in for new services, but has important implications for existing services:

**Non-breaking changes:**
- Existing services continue to work unchanged
- No changes to existing APIs or behavior for services without `@cacheable` traits

**Breaking changes:**
- **Adding `@cacheable` to an existing structure member is a breaking change** for server implementations
- Server handlers that previously expected `T` will now need to handle `Cacheable<T>`
- This requires updating server implementation code to work with the new enum type

### Limitations

#### Clients vs. Servers

This RFC only applies to servers. Clients must ignore the `cacheable` trait.

#### Constraint traits limitation

Currently, `@cacheable` is not supported on shapes that have constraint traits applied. This limitation may be addressed in future iterations of this feature.

#### Cannot be applied to request shapes
To simplify implementation, `@cachable` MUST NOT be used on shapes that are also used by operation inputs. Builders MUST work around this limitation by creating separate shapes for request/responses.

How to actually implement this RFC
----------------------------------

### Create codegen-traits subproject

First, we need to create a new gradle subproject to define server-specific traits:

**Location**: `codegen-traits/`

Create a new gradle subproject with the following structure:
```
codegen-server-traits/
├── build.gradle.kts
└── src/main/resources/META-INF/smithy/
    └── cacheable.smithy
```

**File**: `codegen-traits/src/main/resources/META-INF/smithy/cacheable.smithy`
```smithy
$version: "2"

namespace smithy.rust.codegen.server.traits

@trait(selector: "structure > member, list > member")
structure cacheable {}
```

For the MvP of this feature, the trait will only be applicable to services that use the CBOR protocol. This trait only has an effect when applied to servers. Clients MUST ignore this trait, however, this can be added in the future.

### 2. Generate the Cacheable enum

**Location**: `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/`

When a structure member is marked with `@cacheable`, the code generator will:

1. **Generate the Cacheable enum** in the appropriate module. This will be inlined into the generated code.

```rust
/// Represents a value that can be either fully deserialized or cached in wire format
#[derive(Debug, Clone)]
pub enum Cacheable<T> {
    /// The value has been deserialized into the target type
    Modeled(T),
    /// The value is stored as raw CBOR bytes from the wire
    Cached(bytes::Bytes),
}
```

### Modify structure generation

**Location**: `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerStructureGenerator.kt`

When generating structures, the code generator needs to:

1. **Check for @cacheable trait** on each member
2. **Wrap the field type** in `Cacheable<T>` if the trait is present
3. **Generate appropriate imports** for the Cacheable enum

Example generated structure:
```rust
#[derive(Debug)]
pub struct GetUserResponse {
    pub user_data: Cacheable<UserData>,  // @cacheable applied
    pub request_id: Option<String>,  // No @cacheable trait
}
```

### Generate `to_bytes()` method on target shapes

**Location**: `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerStructureGenerator.kt`

For each shape that is targeted by a `@cacheable` member, generate a `to_bytes()` method:

```rust
impl UserData {
    /// Serialize this value to CBOR bytes for caching
    pub fn to_bytes(&self) -> bytes::Bytes {
        let mut buffer = Vec::new();
        // Delegate to the existing CBOR serializer for this shape
        crate::protocol_serde::shape_user_data::ser_user_data(&mut buffer, self)
            .expect("serialization is infallible");
        bytes::Bytes::from(buffer)
    }
}
```

### 5. Generate validate() method on target shapes

**Location**: `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerStructureGenerator.kt`

For each shape that is targeted by a `@cacheable` member, generate a `validate()` method that validates serialized data:

```rust
impl UserData {
    /// Validate serialized CBOR bytes for this shape
    pub fn validate(bytes: &[u8]) -> Result<(), ValidationError> {
        // Use the generated deserializer to validate the bytes
        let mut reader = std::io::Cursor::new(bytes);
        // NOTE: the simplest version of this is somewhat inefficient because it actually produces the struct & allocates. This should
        // probably only be used in tests / for debug builds for this reason.
        let _: UserData = crate::protocol_serde::shape_user_data::de_user_data(&mut reader)
            .map_err(|e| ValidationError::InvalidData {
                shape: "UserData",
                source: e.into(),
            })?;
        Ok(())
    }
}
```

The `validate` method can be used to validate data integrity:
```rust
let cached_bytes = database.get_cached_response("user_123").await?;

// Validate before using
UserData::validate(&cached_bytes)?;

// Now safe to use as cached data
let response = GetUserResponse {
    user_data: Cacheable::Cached(bytes::Bytes::from(cached_bytes)),
    ..Default::default()
};
```

This method delegates to the existing serialization logic for the shape, ensuring consistency with the protocol implementation.

### Validate a compatible protocol is set:

**Location**: `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/`

Add validation to ensure the `@cacheable` trait is only used with CBOR protocol:

```kotlin
// In the appropriate validator class
if (member.hasTrait(CacheableTrait::class.java)) {
    val protocol = serviceShape.expectTrait(ProtocolsTrait::class.java)
    if (!protocol.protocols.contains(ShapeId.from("smithy.protocols#rpcv2Cbor"))) {
        throw CodegenException(
            "@cacheable trait can only be used with CBOR protocol services"
        )
    }
}
```

### Serializer modifications

This section provides a detailed technical analysis of how wire caching will be implemented at the serialization level, including the necessary modifications to the `CborSerializerGenerator`.

When serializing a struct in `CBOR` a string is written, followed by the value. When writing the `value`, we will inject the raw bytes that have already been framed.

The current CBOR serialization in smithy-rs works as follows:

1. **Function Generation**: `ProtocolFunctions.serializeFn()` generates functions like `ser_user_data()` in modules like `crate::protocol_serde::shape_user_data`
2. **Structure Serialization**: `CborSerializerGenerator.serializeStructure()` generates code that calls `encoder.begin_map()`, serializes each member, then calls `encoder.end()`
3. **Member Serialization**: Each structure member is serialized by calling the appropriate encoder method (`encoder.str()`, `encoder.integer()`, etc.)
4. **Nested Structures**: Nested structures are serialized by calling their respective `ser_*()` functions recursively

#### Wire Caching Implementation Strategy

To implement wire caching, we need to modify the serialization process to:

1. **Detect cacheable members** during structure serialization
2. **Preserve raw CBOR bytes** for cacheable members instead of fully serializing them
3. **Inject cached bytes directly** into the CBOR stream. This requires adding a method to the CBOR encoder to allow direct serialization of pre-serialized data.

There is one more degree of nuance — the `cacheable` trait needs to be ignored by client codegen. This can either be handled in the CBORSerializerGenerator, or by stripping the trait if the protocol is NOT CBOR.

#### Support Cached Bytes in the encoder

The CBOR encoder needs a new method to inject pre-serialized bytes. With this, all public methods add framing.

**Location:** `rust-runtime/aws-smithy-cbor/src/encode.rs`

```rust
impl Encoder {
    /// Write pre-serialized CBOR bytes directly to the output stream
    ///
    /// The caller must ensure that the bytes are a valid segment of CBOR
    pub fn write_preserialized_data(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // Write the bytes directly to the output buffer
        self.writer.writer_mut().write_all(bytes).unwrap();
        Ok(())
    }
}
```

#### Example Generated Code

For a structure like:

```smithy
structure GetUserResponse {
    @cacheable
    userData: UserData,
    requestId: String
}
```

The generated serialization code would look like:

```rust
pub fn ser_get_user_response(
    encoder: &mut Encoder,
    input: &GetUserResponse
) -> Result<(), Error> {
    encoder.begin_map();

    // Serialize cacheable userData field
    match &input.user_data {
        Cacheable::Modeled(inner) => {
            encoder.str("userData");
            crate::protocol_serde::shape_user_data::ser_user_data(encoder, inner)?;
        }
        Cacheable::Cached(bytes) => {
            encoder.str("userData");
            encoder.write_preserialized_data(bytes.as_ref())?;
        }
    }

    // Serialize regular requestId field
    if let Some(var_1) = &input.request_id {
        encoder.str("requestId");
        encoder.str(var_1.as_str());
    }

    encoder.end();
    Ok(())
}
```

### Documentation and examples

**Location**: `design/src/server/` and appropriate documentation locations

Create documentation explaining:
- When to use wire caching
- Risks of wire caching
- Performance implications
- Example usage patterns
- Best practices for cache management

### Testing Strategy

Wire caching requires comprehensive testing:

1. **Round-trip tests**: Serialize → cache → deserialize → verify equality
2. **Mixed mode tests**: Structures with both cached and modeled members
3. **Error condition tests**: Invalid cached bytes, malformed CBOR
4. **Performance tests**: Benchmark cached vs. non-cached serialization
5. **Protocol compliance tests**: Ensure cached output matches protocol specifications

Changes checklist
-----------------

- [ ] Create new `codegen-traits` gradle subproject
- [ ] Define `@cacheable` trait in `codegen-server-traits/src/main/resources/META-INF/smithy/cacheable.smithy`
- [ ] Create `Cacheable<T>` enum
- [ ] Modify `ServerStructureGenerator` to detect `@cacheable` trait and wrap fields in `Cacheable`
- [ ] Update structure field generation to wrap cacheable fields in `Cacheable<T>`
- [ ] Generate `to_bytes()` method on target shapes that delegates to existing shape serializers
- [ ] Generate `validate()` method on target shapes that delegates to existing shape deserializers
- [ ] Add protocol validation to restrict `@cacheable` to CBOR services only
- [ ] Create comprehensive tests for wire caching functionality
- [ ] Add integration tests with CBOR protocol
- [ ] Write documentation explaining breaking change implications
- [ ] Document migration path for adding `@cacheable` to existing services
- [ ] Add performance benchmarks comparing cached vs. non-cached scenarios
- [ ] Create examples showing server-side usage patterns
