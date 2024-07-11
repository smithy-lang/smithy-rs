<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Your RFC
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: client, server

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines how smithy-rs will enable customers to use the [serde](serde.rs) library with generated clients & servers. This is a common request
for myriad reasons, but as we have written about [before](https://github.com/awslabs/aws-sdk-rust/issues/269#issuecomment-1227518721) this is a challenging design area. This RFC proposes a new approach: **Rather than implement `Serialize` directly, add a method to types that returns a type that implements `Serialize`. This solves a number of issues:
    1. It doesn't lock us into one `Serialize` implementation.
    2. It allows customers to configure serde to their use case. For example, for testing/replay you probably _don't_ want to redact sensitive fields but for logging or other forms of data storage, you may want to redact those fields.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **Some Term**: A definition for that term

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

Generated crates will include a `serde` feature. This will feature gate the module containing the serialization logic. This will enable an extension trait which provides two methods:

```rust
my_thing.serialize_ref(&settings);
my_thing.serialize_owned(settings);
```

Both of these methods return a struct that implements `Serialize`.



### Customer Use Cases
#### I want to embed a structure into my own types that implement `Serialize`
The generated code includes two methods: 
`serialize_redacted` and `serialize_unredacted`.

These have the correct signatures to be used with `serialize_with`:
```rust
#[derive(Serialize)]
struct MyStruct {
    #[serde(serialize_with = "serialize_redacted")]
    inner: SayHelloInput,
}
```
2. I want to serialize data for testing
3. I want to dump a structured form of data into a database/logs/etc.

In the current version of the SDK, users do X like this...
Once this RFC is implemented, users will do X like this instead...

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

In order to implement this feature, we need to add X and update Y...

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [x] Create new struct `NewFeature`
