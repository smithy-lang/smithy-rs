<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Serialization and Deserialization
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: builder structs and other data types that are connected to builder structs

# Overview
There has been some github issues requesting for the implementation of Serialize/Deserialize traits provided serde crate.  
At the current state of sdk, user must implement their own solution in order to serialize/deserialize sdk's datatypes.  
This is a RFC for implementing serde traits.

# Potential Use Cases
- Testing
- Reusable conifiguration, infrastructure as a service
- Logging
 
... and more!

# Implementation
Add `serde::Serialize` and `serde::Deserialize` to builder datatypes.

# Discussion
## Enum Representation 
serde allows programmers to use one of four different tagging ([internal, external, adjacent and untagged](https://serde.rs/enum-representations.html)) when serializing enum.  

I suggest going with `untagged` variant.  
This way serialized data can be used with aws-cli's file-based input.  

## Backwards Compatability
- When new fields are added to builder  
Fields of builder structs are always Option\<T>.

When serde is unable to find a field when deserializing the data, it assigns `None` to the corresponding field.  
It is highly unlikely that deserialization of data to fail when new fields are introduced to builder types.

- When fields are removed from builder   
Users will not be able to access that removed fields via builder, however, users should be able to access it via different datatype. (e.g. `serde_json::Map`)

- feature gate for deriving `serde::Serialize` and `serde::Deserialize`
We are going to implement them behind a feature-gate.  
The name of the feature gate is going to be `unstable-serde-de` and `unstable-serde-ser`.
```rust
#[cfg_attr(feature = "unstable-serde-ser"), derive(serde::Serialize)]
#[cfg_attr(feature = "unstable-serde-de"), derive(serde::Deserialize)]
```

This allow users to have better control of their compile time when compared to implementing them both on behind a same feature-gate.

## implementing `serde::Serialize` and `serde::Deserialize` on builder types

I am going to implement serde traits on existing builder types, instead of introducing new data types for serialization/deserialization.

Things that I considered
- Introduction of new data types will be more complicated; This will require an implementation of a function which converts the new data type into an corresponding data type.
- Compile time
Introduction of new data type should result in longer compile time than the other way around.
This one is based on my unscientific assumption; No testing has been done to verify this.
- Binary size
Binary size should become bigger, however, it should be managible.
 

## Testing
Every data format is different, and we saw that serialization of enum did not work on some cases.  
Ideally, we want every data format to work with the sdk, however,
- there are large number of data format out there and not every one of them are implemented for serde
- even when serde implementations are available, some of them require programmers to pass additional pieces of data (e.g. [apache-avro](https://docs.rs/apache-avro/latest/apache_avro/) requires programmers to provide schema definition) which requires considerable investment in time to test out
- You can't simply map rust lang's data in some cases e.g. XML, CSV.  
  To clearify, it is technicalliy possible to implement something that works with XML and CSV, but there are things to figure out, for example
    - in case of CSV, you need to figure out how to handle nested structures
    - in case of XML, you need to figure out what to do with attributes, child tags, contents ... etc

Having said, I suggest starting with following data format.
- JSON
- YAML
- TOML
  
Those data formats are popular and it puts strong emphasis on human-readability.  

Since we could use serialized data used in testing for examples for new users, I think this is a fair start.  
Also, many popular data format should work if it is going to work with these 3. (I have no scientific basis for this. It's just my gut feeling.)

Changes checklist
-----------------

- [ ] Add `#[cfg_attr(feature = "unstable-serde-ser"), derive(serde::Serialize)]` to builder types
- [ ] Add `#[cfg_attr(feature = "unstable-serde-de"), derive(serde::Deserialize)]` to builder types
- [ ] Implement tests for above mentioned data format
- [ ] Prepare files/string for testing deserialization/serialization