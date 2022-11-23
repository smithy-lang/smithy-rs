<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Serialization and Deserialization
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: Builder data types, data types that builder types may hold as a member as well as three data types `DateTime`, `Document`, `Blob` and `Number` implemented in `aws_smithy_types` crate.

# Overview
We are going to implement Serialize and Deserialize traits from serde crate on some data types.   
Data types are, a) builder data types and b) data types that builder types may have on its field(s) and c) `DateTime`, `Document`, `Blob` and `Number` implemented in `aws_smithy_types` crate.

`DateTime` and `Blob` implements different serialization/deserialization format for human-readable and non-human redable format.

# Use Case
Users have requested `serde` traits to be implemented on data types implemented in rust SDK.  
We have created this RFC with following use cases in mind.
1. [[request]: Serialize/Deserialize of models for Lambda events #269](https://github.com/awslabs/aws-sdk-rust/issues/269)
2. [Tests](https://awslabs.github.io/smithy-rs/design/faq.html#why-dont-the-sdk-service-crates-implement-serdeserialize-or-serdedeserialize-for-any-types) as suggested in the design FAQ.
3. Use of configuration files for filling parameters

# Implementation
## Smithy Types
`aws_smithy_types` is a crate that implements smithy's data types.  
These data types must implement serde traits as well since SDK uses the data types.

### Blob
`Serialize` and `Deserialize` is not implemented with derive macro.  

In human-readable format, `Blob` is serialized as base64 encoded string and any data to be deserialized as this data type must be encoded in base 64.  
Encoding must be carried out by `base64::encode` function available from `aws_smithy_types` crate.  
Non-human readable format implements `Blob` as an array of `u64` which is created by encoding `u8` as big endian.  

- Reason behind the implementation of human-readable format
 
`aws_smithy_types` crate comes with functions for encoding base 64, which makes the implementation simpler.  
Additionally, AWS CLI and AWS SDK for other languages require data to be encoded in base 64 when it requires `Blob` type as input.  

For the reasons above, we believe base 64 is favourable over other encoding schemes such as base 16, 32, or Ascii85.  

- Reason behind the implementation of non-human readable format

We can make the resulting size of the serialized data smaller.  
Alternatively, we can serialize it as an array of `u8`, which will make the implementation simpler.
However, we believe that the benefit outweighs the complexity.

For encoding, we considered using little endian, however, we believe that there is no advantage or disadvantage over one another.  

### Date Time
`Serialize` and `Deserialize` is not implemented with derive macro.  
For human-readable format, `DateTime` is serialized in RFC-3339 format; 
It expects the value to be in RFC-3339 format when it is Deserialized.  

Non-human readable implements `DateTime` as a tuple of `u32` and `i64`; the latter corresponds to `seconds` field and the first is the `seubsecond_nanos`.

- Reason behind the implementation of human-readable format

For serialization, `DateTime` format already implements a function to encode itself into RFC-3339 format.   
For deserialization, it is possible to accept other formats, we can add this later if we find it reasonable.   

- Reason behind the implementation of a non-human readable format

Serializing them as tuples of two integers results in smaller data size  and requires less computing power than any string-based format.  
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
We considered implementing manually or implementing them as part of code-gen; We are not able to find any advantage that justifies the complexity.

## Enum Representation 
`serde` allows programmers to use one of four different tagging ([internal, external, adjacent and untagged](https://serde.rs/enum-representations.html)) when serializing enum.  
## untagged  
  You cannot deserialize serialized data in some cases. 
  For example, [aws_sdk_dynamodb::model::AttributeValue](https://docs.rs/aws-sdk-dynamodb/latest/aws_sdk_dynamodb/model/enum.AttributeValue.html) has `Null(bool)` and `Bool(bool)`, which you cannot distinguish serialized values without a tag.

  
## internal  
  This results in compile time error.
  [*Using a #[serde(tag = "...")] attribute on an enum containing a tuple variant is an error at compile time*](https://serde.rs/enum-representations.html).

## external and adjacent
We are left with `external` and `adjacent` tagging.  
External tagging is the default way.  
This RFC can be achieved in either way.  

The resulting size of the serialized data is smaller when tagged externally, as adjacent tagging will require an tag even when a variant has no content.

For the reasons mentioned above, we implement enum is externally tagged.

# What users must know
## Sensitive Information
Serialized data may involve sensitive information.  

## 

## Compile Time
// TODO   
// NOTE we could create a dynamically/statically linked binary
## Misleading Results
// TODO

# Feature Gate
Features suggested in this RFC are implemented behind feature gates.  
`Serialize`  is implemented behind `unstable-serde-serialize`.
`Deserialize` is implemented behind `unstable-serde-deserialize`.

We considered following alternatives,
## Keeping both features behind same feature gate
We considered keeping both features behind same feature gate.  
We are able to reduce the complexity of the implementation by keeping both features behind same feature gate.  
This will result in increase in compilation time when users are not in need of one of the features, and compelxity of we will be able to reduce.

We believe that the benefit outwieghs the additional complexity, thus we decoded to implement two feature gate.

## Different feature gate for different data types
We considered implementing different feature gate for output, input and it's corresponding data types.
For example, output and input types can have `unstable-output-serde-*` and `unstable-input-serde-*`.

Complexity that this implementation introduces is significant as data types in Kotlin does not hold any meta data that determines which one of the category that data belongs too.
Thus, we believe that benefit does not outweigh the cost of maintenance and implementation.


Changes checklist
-----------------
- [ ] Implement human-redable serialization for `DateTime` and `Blob` in `aws_smithy_types`
- [ ] Implement non-human-redable serialization for `DateTime` and `Blob` in `aws_smithy_types`
- [ ] Implement `Serialize` and `Deserialize` for relevant data types in `aws_smithy_types`
- [ ] Modify Kotlin's codegen to so that generated Builder and non-Builder types implements `Serialize` and `Deserialize`
- [ ] Add feature gate for `Serialize` and `Deserialize`
- [ ] Prepare examples
- [ ] Prepare reproducible compile time benchmark