RFC: Supporting `SelectObjectContent`'s unique `ScanRange` behavior
=============

> Status: RFC
>
> Applies to: AWS client

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

The Rust SDK does not currently support all the documented behavior for the `ScanRange` input field of the
`SelectObjectContent` operation. This RFC defines why and how we should support this behavior.

Terminology
-----------

- [**Smithy IDL v2**][IDLv2]: The latest version of the Smithy modeling language. AWS services are defined with Smithy,
  and we use it to generate the SDKs.
- [**The `@default` trait**][default-trait]: A Smithy trait that allows modelers to define a default value for a struct member.
- **Service Customizations**: Customizations modify the process of code generation, allowing for the inserting code into
  pre-defined locations in the `config`, `operation`, or other modules. Service customizations are customization that
  are specific to a single AWS service.
- [**SelectObjectContent**][SelectObjectContent]: This action filters the contents of an Amazon S3 object based on a
  simple structured query language (SQL) statement.
- [**ScanRange**][ScanRange]: Used as an input to `SelectObjectContent`. Specifies the byte range of the object to get
  the records from. Its functioning is based on [RFC 2616, Section 14.35.1].
- **unset / unsetting**: The act of explicitly setting a value, which normally has a default, to be `None`.

Why this RFC should be accepted
-------------------------------

There is currently one [user-submitted issue] in the `aws-sdk-rust` asking us to implement this behavior.

TODO figure out what other SDKs support this behavior

The user experience if this RFC is implemented
----------------------------------------------

### In Smithy IDLv1 (the current implementation)

SDKs built from IDLv1 Smithy models intentionally omit values from a request if a field has a default and the
user-provided value is equal to that default:

```smithy
namespace aws.example

operation SomeOp {
    input: OpInput
}

structure OpInput {
    @default(0)
    aField: PrimitiveInt
}
```

```rust
async fn example() {
    let req = client.some_op()
        .a_field(0)
        .customize()
        .await
        .request();

    assert!(req.headers().get("a_field").is_none());

    let req = client.some_op()
        .a_field(1)
        .customize()
        .await
        .request();

    assert_eq!(req.headers().get("a_field").unwrap(), 1);
}
```

**In the current version of the SDK,** if a field defines a default, it's impossible for users to explicitly unset that
field. Additionally, users may be surprised to learn that passing a default value effectively acts the same as
"unsetting" value because of the behavior demonstrated above.

### In Smithy IDLv2 (the future implementation)

In [IDLv2], users will be able to explicitly "unset" fields that define a default value. However, `ScanRange` is defined
with defaults that won't work as [documented][ScanRange]:

```json
{
  "com.amazonaws.s3#ScanRange": {
    "type": "structure",
    "members": {
      "Start": {
        "target": "com.amazonaws.s3#Start",
        "traits": {
          "smithy.api#default": 0,
          "smithy.api#documentation": "<p>Specifies the start of the byte range. This parameter is optional. Valid values:\n         non-negative integers. The default value is 0. If only <code>start</code> is supplied, it\n         means scan from that point to the end of the file. For example,\n            <code><scanrange><start>50</start></scanrange></code> means scan\n         from byte 50 until the end of the file.</p>"
        }
      },
      "End": {
        "target": "com.amazonaws.s3#End",
        "traits": {
          "smithy.api#default": 0,
          "smithy.api#documentation": "<p>Specifies the end of the byte range. This parameter is optional. Valid values:\n         non-negative integers. The default value is one less than the size of the object being\n         queried. If only the End parameter is supplied, it is interpreted to mean scan the last N\n         bytes of the file. For example,\n            <code><scanrange><end>50</end></scanrange></code> means scan the\n         last 50 bytes.</p>"
        }
      }
    },
    "traits": {
      "smithy.api#documentation": "<p>Specifies the byte range of the object to get the records from. A record is processed\n         when its first byte is contained by the range. This parameter is optional, but when\n         specified, it must not be empty. See RFC 2616, Section 14.35.1 about how to specify the\n         start and end of the range.</p>"
    }
  }
}
```

Additionally, the `@default` trait doesn't _(and likely will never)_ support the ability to define a dynamic value like
the _"one less than the size of the object being queried"_ behavior that the docs say `end` is meant to default to.

**Once this RFC is implemented,** users will finally be able to use the `ScanRange` input as documented:
- Default values will be equal to those specified in the `ScanRange` docs.
    - For `start`, the default value is zero.
    - For `end`, the default value is one less than the size of the object being queried.
- If `start` is set and `end` is "unset", the object will be scanned from `start` to then end of the file.
- If `end` is set and `start` is "unset", the last `N` bytes of the object will be read, where `N == end`.
- If both fields are set, the object will be scanned from `start` to `end`
- If both fields are "unset", S3 will respond with an error because at least one must be set.

How to actually implement this RFC
----------------------------------

In order to implement this feature, we would extend the current S3 customization to prevent `ScanRange` from being
generated normally and would instead insert inlineable code into `aws_sdk_s3::models` that provided a handwritten
version of `ScanRange` with special behavior. We would generate a serialization function as well that handles
defaults and "unset" fields as defined in the `ScanRange` docs.

By generating an enum instead of a struct, we could allow users to be explicit about the behavior they expect.

```rust
#[non_exhaustive]
#[derive(Clone, PartialEq)]
pub enum ScanRange {
    StartToEnd {
        start: i64,
        end: i64,
    },
    StartToEndOfObject {
        start: i64,
    },
    LastNBytesOfObject {
        n_bytes: i64
    }
}

impl ScanRange {
    pub fn start_to_end(start: i64, end: i64) -> Self {
        ScanRange::StartToEnd { start, end }
    }

    pub fn start_to_end_of_object(start: i64) -> Self {
        ScanRange::StartToEndOfObject { start }
    }

    pub fn last_n_bytes_of_object(n_bytes: i64) -> Self {
      ScanRange::LastNBytesOfObject { n_bytes }
    }

    pub fn start(&self) -> Option<i64> {
        match self {
            ScanRange::StartToEnd { start, .. } => Some(start),
            ScanRange::StartToEndOfObject { start } => Some(start),
            ScanRange::LastNBytesOfObject { .. } => None,
        }
    }

    pub fn end(&self) -> Option<i64> {
        match self {
            ScanRange::StartToEnd { end, .. } => Some(end),
            ScanRange::StartToEndOfObject { .. } => None,
            ScanRange::LastNBytesOfObject { n_bytes } => Some(n_bytes),
        }
    }
}
```

To ensure that this handwritten struct doesn't deviate from the S3 model, we'd create a codegen test that would verify
compatibility of the two models.

Why this RFC should be rejected
-------------------------------

There are several reasons to reject this RFC:

- Writing customizations to make up for the modeling mistakes of services is not a scalable strategy. Any customizations
  we write won't necessarily be implemented by other SDKs, causing a fragmented and annoying user experience.
- The proposed behavior is confusing and difficult to explain.
- Differentiating between "set", "default", and "unset" struct fields is not currently supported by the SDK. Adding it
  would take a few weeks of effort. _(We do need to eventually support this anyway for IDL v2.)_
- [Only one user][user-submitted issue] has asked for this feature at the time of writing this RFC.

### What we should do instead

We have already committed to supporting [IDLv2] and implementing that would allow users to set `start=0` or unset it.
Although IDLv2 support would NOT support the custom `end` value behavior, we'd be halfway there. Any future S3
customization would only have to customize the `end` field of `ScanRange`, reducing the burden of implementation and
maintenance.

Changes checklist
-----------------

- [ ] Create new customization `S3ScanRange`
- [ ] Create test ensuring handwritten `ScanRange` stays in-sync with the modeled `ScanRange`

[IDLv2]: https://smithy.io/2.0/index.html
[default-trait]: https://smithy.io/2.0/spec/type-refinement-traits.html?highlight=default
[SelectObjectContent]: https://docs.aws.amazon.com/AmazonS3/latest/API/API_SelectObjectContent.html
[ScanRange]: https://docs.aws.amazon.com/AmazonS3/latest/API/API_ScanRange.html
[RFC 2616, Section 14.35.1]: https://www.rfc-editor.org/rfc/rfc2616.html#section-14.35.1
[user-submitted issue]: https://github.com/awslabs/aws-sdk-rust/issues/630
