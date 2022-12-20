RFC: Serialization and Deserialization
=============

> Status: RFC
>
> Applies to: Builder data types, data types that builder types may hold as a member as well as three data types `DateTime`, `Document`, `Blob` and `Number` implemented in `aws_smithy_types` crate.

# Terminology
- Builder  
 Refers to data types prefixed with `Builder`, which converts itself into a corresponding data type upon being built. e.g. `aws_sdk_dynamodb::input::PutItemInput`.
- serde  
  Refers to `serde` crate.
- `Serialize`   
  Refers to `Serialize` trait avaialble on `serde` crate.
- `Deserialize`  
  Refers to `Deserialize` trait available on `serde` crate.
  

# Overview
We are going to implement Serialize and Deserialize traits from serde crate on some data types.   
Data types are, 
- builder data types
- data types that builder types may have on its field(s)
- `aws_smithy_types::DateTime`
- `aws_smithy_types::Document`
- `aws_smithy_types::Blob`
- `aws_smithy_types::Number`

`DateTime` and `Blob` implements different serialization/deserialization format for human-readable and non-human readable format.

Additionally, we add `fn set_fields` to fluent builders to allow users to set the data they deserialized to fluent builders.

Lastly, we emphasize that this RFC does NOT aim to serialize the entire response or request.

# Use Case
Users have requested `serde` traits to be implemented on data types implemented in rust SDK.  
We have created this RFC with the following use cases in mind.
1. [[request]: Serialize/Deserialize of models for Lambda events #269](https://github.com/awslabs/aws-sdk-rust/issues/269)
2. [Tests](https://awslabs.github.io/smithy-rs/design/faq.html#why-dont-the-sdk-service-crates-implement-serdeserialize-or-serdedeserialize-for-any-types) as suggested in the design FAQ.
3. Building tools

# Implementation
## Smithy Types
`aws_smithy_types` is a crate that implements smithy's data types.  
These data types must implement serde traits as well since SDK uses the data types.

### Blob
`Serialize` and `Deserialize` is not implemented with derive macro.  

In human-readable format, `Blob` is serialized as a base64 encoded string and any data to be deserialized as this data type must be encoded in base 64.  
Encoding must be carried out by `base64::encode` function available from `aws_smithy_types` crate.  
Non-human readable format serializes `Blob` with `fn serialize_bytes`.

- Reason behind the implementation of human-readable format
 
`aws_smithy_types` crate comes with functions for encoding/decoding base 64, which makes the implementation simpler.  
Additionally, AWS CLI and AWS SDK for other languages require data to be encoded in base 64 when it requires `Blob` type as input.  

We also considered serializing them with `serialize_bytes`, without encoding them with `serialize_bytes`.
In this case, the implementation will depend on the implementation of the library author.

There are many different crates, so we decided to survey how some of the most popular crates implement this feature.

| library    | version | implementation  | all time downloads on crate.io as of writing (Dec, 2022) |
| ---------- | ------- | --------------- | -------------------------------------------------------- |
| serde_json | 1.0     | Array of number | 109,491,713                                              |
| toml        | 0.5.9   | Array of number | 63,601,994                                               |
| serde_yaml | 0.9.14  | Unsupported     | 23,767,300                                               |

First of all, bytes could have hundreds of elements; reading an array of hundred of numbers will never be a pleasing experience, and it is especially troubling when you are writing data for test cases.
Additionally, it has come to our attention that some crates just doesn't support it, which would hinder users ability to be productive and tie users' hand.

For the reasons described above, we believe that it is crucial to encode them to string and base64 is favourable over other encoding schemes such as base 16, 32, or Ascii85.  

- Reason behind the implementation of non-human readable format
We considered using the same logic for non-human readable format as well.
However, readable-ness is not necessary for non-human readable format.
Additionally, non-human readable format tends to emphasize resource efficiency over human-readable format; Base64 encoded string would take up more space, which is not what the users would want.

Thus, we believe that implementing a tailored serialization logic would be beneficial to the users.


### Date Time
`Serialize` and `Deserialize` is not implemented with derive macro.  
For human-readable format, `DateTime` is serialized in RFC-3339 format; 
It expects the value to be in RFC-3339 format when it is Deserialized.  

Non-human readable implements `DateTime` as a tuple of `u32` and `i64`; the latter corresponds to `seconds` field and the first is the `seubsecond_nanos`.

- Reason behind the implementation of a human-readable format

For serialization, `DateTime` format already implements a function to encode itself into RFC-3339 format.   
For deserialization, it is possible to accept other formats, we can add this later if we find it reasonable.   

- Reason behind the implementation of a non-human readable format

Serializing them as tuples of two integers results in a smaller data size and requires less computing power than any string-based format.  
Tuple will be smaller in size as it does not require tagging like in maps.  

### Document
`Serialize` and `Deserialize` is implemented with derive macro.
Additionally, it implements container attribute `#[serde(untagged)]`.  
Serde can distinguish each variant without tagging thanks to the difference in each variant's datatypes.   

### Number
`Serialize` and `Deserialize` is implemented with derive macro.
Additionally, it implements container attribute `#[serde(untagged)]`.  

Serde can distinguish each variant without a tag as each variant's content is different.   

## Builder Types and Non-Builder Types
Builder types and non Builder types implement `Serialize` and `Deserialize` with derive macro.
Derive macro will be implemented behind a feature-gate; Users must enable `serialize` to use `serde::Serialize`, and `deserialize` to use `serde::Deserialize` respectively. Additionally, user must enable `unstable` until the stablization of this RFC.

```rust
#[allow(missing_docs)] // documentation missing in model
#[cfg_attr(
    all(feature = "unstable", feature = "serialize"),
    derive(serde::Serialize)
)]
#[cfg_attr(
    all(feature = "unstable", feature = "deserialize"),
    derive(serde::Deserialize)
)]
#[non_exhaustive]
#[derive(std::clone::Clone, std::cmp::PartialEq)]
pub struct UploadPartCopyOutput {
  ...
}
```

## Enum Representation 
`serde` allows programmers to use one of four different tagging ([internal, external, adjacent and untagged](https://serde.rs/enum-representations.html)) when serializing an enum.  
### untagged  
  You cannot deserialize serialized data in some cases. 
  For example, [aws_sdk_dynamodb::model::AttributeValue](https://docs.rs/aws-sdk-dynamodb/latest/aws_sdk_dynamodb/model/enum.AttributeValue.html) has `Null(bool)` and `Bool(bool)`, which you cannot distinguish serialized values without a tag.

  
### internal  
  This results in compile time error.
  [*Using a #[serde(tag = "...")] attribute on an enum containing a tuple variant is an error at compile time*](https://serde.rs/enum-representations.html).

### external and adjacent
We are left with `external` and `adjacent` tagging.  
External tagging is the default way.  
This RFC can be achieved either way.  

The resulting size of the serialized data is smaller when tagged externally, as adjacent tagging will require a tag even when a variant has no content.

For the reasons mentioned above, we implement an enum that is externally tagged.

## Data Types to Skip
We are going to skip serialization and deserialization of fields that have the following datatypes.

- `aws_smithy_http::byte_stream::ByteStream`
- `aws_smithy_http::event_stream::Receiver<TranscriptResultStream, TranscriptResultStreamError>:`
- `aws_smithy_http::event_stream::EventStreamSender<AudioStream, AudioStreamError>`

Any fields with these data types are tagged with `#[serde(skip_serialization)]` and `#[serde(skip_deserialization)]`.

Here are some examples of data types affected by this decision:
- `aws_sdk_transcribestreaming::client::fluent_builders::StartMedicalStreamTranscription`
- `aws_sdk_s3::input::put_object_input::PutObjectInput`

We considered serializing them as bytes, however, it could take some time for a stream to reach the end, and the resulting serialized data may be too big for itself to fit into the ram.   
Additionally, those data types are sometimes used to represent bi-directional data transfer, which is not serializable.

## `Serde` traits implemented on Builder of Output Types
Output data, such as `aws_sdk_dynamodb::output::UpdateTableOutput` has builder types. 
These builder types are available to users, however, no API requires users to build data types by themselves.  

We considered removing traits from these data types, however, the code-gen framework does not carry the necessary metadata to determine whether the data is the builder type of an output type or not.
We conclude that we must avoid such a technical challenge to bring this RFC to life.

## `fn set_fields` to allow users supply externally created `Input`s

SDK does not have a method that allows users supply deserialized inputs.
Thus, we add a new method `fn set_fields` to `Client` types.
This method accepts inputs and replace all parameters that `Client` has with the new one.

```rust
pub fn set_fields(mut self, builder: path::to::builder_type) -> path::to::builder_type {
    self.inner = new_parameter;
    self
}
```

# Other Concerns
## Model evolution 
SDK will introduce new fields and it is possible that we may see new data types in future.  

We believe that this will not be a problem.

### Introduction of New Fields
Most fields are `Option<T>` type. 
When user de-serializes data written for a format before the new fields were introduced, new field will be assigned with `None` type.

If a field isn't `Option`, `serde` uses `Default` trait unless a custom de-serialization/serialization is specified to generate data to fill the field.  
If the new field is not an `Option<T>` type and has no `Default` implementation, we must implement a custom de-serialization logic.

In case of serilization, introduction of new fields will not be an issue unless the data format requires a schema. (e.g. parquet, avro) However, this is outside the scope of this RFC.

## Introduction of New Data Type
If a new field introduces a new data type, it will not require any additional work if the data type can derive `serde` traits.  

If the data cannot derive `serde` traits on it's own, then we have two options.
To clarify, this is the same approach we took on `Data Type to skip` section.
1. skip
   We will simply skip serializing/de-serializing. However, we may need to implement custom serialization/de-serialization logic if a value is not wrapped with `Option`.
2. custom serialization/de-serialization logic 
   We can implement tailored serialization/de-serialization logic.

Either way, we will mention this on the generated docs to avoid suprising users.

e.g.
```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct OutputV1 {
  string_field: Option<String>
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OutputV2 {
  string_field: Option<String>,
  // this will always be treated as None value by serde
  #[serde(skip)] 
  skip_not_serializable: Option<SomeComplexDataType>,
  // We can implement a custom serialization logic
  #[serde(serialize_with = "custom_serilization_logic", deserialize_with = "custom_deserilization_logic")]
  not_derive_able: SomeComplexDataType,
  // Serialization will be skipped, and de-serialization will be handled with the function provided on default tag
  #[serde(skip, default = "default_value")]
  skip_with_custom: DataTypeWithoutDefaultTrait,
}
```
# Discussions

## Serialization and de-serialization support for an entire response/request
The problem with serialization/de-serialization of an entire response/request the lack of data type that can be mapped to `serde`'s data model field by field.

Currently, SDK has no data type that represents an entire response or request that can be mapped to `serde`'s data model; Thus, you must introduce a schema and implement logics that allows users to serialize/de-serialize their data.

Although this RFC does not solve this issue, we believe that this RFC will help future contirbutor who wishes to implement serialization and de-serialization support for an entire response/request.


## Sensitive Information
If serialized data contains sensitive information, it will not be masked.  
Users must be cautious to avoid compromisation

## Compile Time
We ran the following benchmark on C6a.2xlarge instance with 50gb of GP2 SSD.  
The commit hash of the code is a8e2e19129aead4fbc8cf0e3d34df0188a62de9f.

It clearly shows an increase in compile time.  
Users are advised to consider the use of software such as [sccache](https://github.com/mozilla/sccache) or [mold](https://github.com/rui314/mold) to reduce the compile time.

- `aws-sdk-dynamodb`
 
| command                                                     | real time | user time | sys time  |
| ----------------------------------------------------------- | --------- | --------- | --------- |
| cargo build                                                 | 0m35.728s | 2m24.243s | 0m11.868s |
| cargo build --release                                       | 0m52.040s | 5m0.841s  | 0m11.313s |
| cargo build --features unstable-serde-serialize             | 0m38.079s | 2m26.082s | 0m11.631s |
| cargo build --release --features unstable-serde-serialize   | 0m53.153s | 5m4.069s  | 0m11.577s |
| cargo build --features unstable-serde-deserialize           | 0m45.689s | 2m34.000s | 0m11.978s |
| cargo build --release --features unstable-serde-deserialize | 1m0.107s  | 5m10.231s | 0m11.699s |
| cargo build --all-features                                  | 0m48.959s | 2m45.688s | 0m13.359s |
| cargo build --release --all-features                        | 1m3.198s  | 5m26.076s | 0m12.311s |

- `aws-sdk-ec2`
 
| command                                                     | real time | user time  | sys time  |
| ----------------------------------------------------------- | --------- | ---------- | --------- |
| cargo build                                                 | 1m20.041s | 2m14.592s  | 0m6.611s  |
| cargo build --release                                       | 2m29.480s | 9m19.530s  | 0m15.957s |
| cargo build --features unstable-serde-serialize             | 2m0.555s  | 4m24.881s  | 0m16.131s |
| cargo build --release --features unstable-serde-serialize   | 2m45.002s | 9m43.098s  | 0m16.886s |
| cargo build --features unstable-serde-deserialize           | 3m10.857s | 5m34.246s  | 0m18.844s |
| cargo build --release --features unstable-serde-deserialize | 3m47.531s | 10m52.017s | 0m18.404s |
| cargo build --all-features                                  | 3m31.473s | 6m1.052s   | 0m19.681s |
| cargo build --release --all-features                        | 3m45.208s | 8m46.168s  | 0m10.211s |


## Misleading Results
SDK team previously expressed concern that serialized data may be misleading.  
We believe that features implemented as part of this RFC does not produce misleading result as we focus on builder types and it's corresponding data types which are mapped to serde's data type model with the derive macro.  

# Feature Gate
`Serde` traits are implemented behind feature gates. 
`Serialize`  is implemented behind `serialize`., while `Deserialize` is implemented behind  `deserialize`.
Users must enable `unstable` feature to expose those features.

We considered giving each feature a dedicated feature gate such as `unstable-serde-serialize`. 
In this case, we will need to change the name of feature gates entirely once it leaves the unstable status which will cause users to make changes to their code base.
We conclude that this brings no benefit to the users.

## Keeping both features behind the same feature gate
We considered keeping both features behind the same feature gate.  
There is no significant difference in the complexity of implementation.
We do not see any benefit in keeping them behind a same feature-gate as this will only result in increase of compile time when users are not in need of one of the feature.

## Different feature gates for different data types
We considered implementing different feature gates for output, input and their corresponding data types.
For example, output and input types can have `output-serde-*` and `input-serde-*`.
We are unable to do this as relevant meta data is not available during the code-gen.


Changes checklist
-----------------
- [ ] Implement human-redable serialization for `DateTime` and `Blob` in `aws_smithy_types`
- [ ] Implement non-human-redable serialization for `DateTime` and `Blob` in `aws_smithy_types`
- [ ] Implement `Serialize` and `Deserialize` for relevant data types in `aws_smithy_types`
- [ ] Modify Kotlin's codegen so that generated Builder and non-Builder types implement `Serialize` and `Deserialize`
- [ ] Add feature gate for `Serialize` and `Deserialize`
- [ ] Prepare examples
- [ ] Prepare reproducible compile time benchmark