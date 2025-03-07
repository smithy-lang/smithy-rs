<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: The HTTP Wrapper Type
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: client

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines the API of our wrapper types around `http::Request` and `http::Response`. For more information about why we are wrapping these types, see [RFC 0036: The HTTP Dependency](./rfc0036_http_dep_elimination.md).

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------
- `Extensions` / "Request Extensions": The `http` crate Request/Response types include a typed property bag to store additional metadata along with the request.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

In the current version of the SDK, external customers and internal code interacts directly with the [`http`](https://crates.io/crates/http) crate. Once this RFC is implemented, interactions at the **public** API level will occur with our own `http` types instead.

Our types aim to be nearly drop-in-compatible for types in the `http` crate, however:
1. We will not expose existing HTTP types in public APIs in ways that are ossified.
2. When possible, we aim to simplify the APIs to make them easier to use.
3. We will add SDK specific helper functionality when appropriate, e.g. first-level support for applying an endpoint to a request.

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

We will need to add two types, `HttpRequest` and `HttpResponse`.

#### To string or not to String
Our header library restricts header names and values to `String`s (UTF-8).

Although the `http` library is very precise in its representation—it allows for `HeaderValue`s that are both a super and subset of `String`—a superset because headers support arbitrary binary data but a subset because headers cannot contain control characters like `\n`.

Although technically allowed, headers containing arbitrary binary data are not widely supported. Generally, Smithy protocols will use base-64 encoding when storing binary data in headers.

Finally, it's nicer for users if they can stay in "string land". Because of this, HttpRequest and Response expose header names and values as strings. Internally, the current design uses `HeaderName` and `HeaderValue`, however, there is a gate on construction that enforces that values are valid UTF-8.

**This is a one way door because `.as_str()` would panic in the future if we allow non-string values into headers.**

#### Where should these types live?
These types will be used by all orchestrator functionality, so they will be housed in `aws-smithy-runtime-api`

#### What's in and what's out?
At the onset, these types focus on supporting the most ossified usages: `&mut` modification of HTTP types. They **do not**
support construction of HTTP types, other than `impl From<http::Request>` and `From<http::Response>`. We will also make it
possible to use `http::HeaderName` / `http::HeaderValue` in a zero-cost way.

#### The `AsHeaderComponent` trait
All header insertion methods accept `impl AsHeaderComponent`. This allows us to provide a nice user experience while taking
advantage of zero-cost usage of `'static str`. We will seal this trait to prevent external usage. We will have separate implementation for:
- `&'static str`
- `String`
- http02x::HeaderName

#### Additional Functionality
Our wrapper type will add the following additional functionality:

1. Support for `self.try_clone()`
2. Support for `&mut self.apply_endpoint(...)`

#### Handling failure
There is no stdlib type that cleanly defines what may be placed into headers—String is too broad (even if we restrict to ASCII). This RFC proposes moving fallibility to the APIs:
```rust,ignore
impl HeadersMut<'_> {
    pub fn try_insert(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Result<Option<String>, BoxError> {
        // ...
    }
}
```

This allows us to offer user-friendly types while still avoiding runtime panics. We also offer `insert` and `append` which panic on invalid values.

#### Request Extensions
There is ongoing work which MAY restrict HTTP extensions to clone types. We will preempt that by:
1. Preventing `Extensions` from being present when initially constructing our HTTP request wrapper.
2. Forbidding non-clone extensions from being inserted into the wrapped request.

This also enables supporting request extensions for different downstream providers by allowing cloning into different extension types.

#### Proposed Implementation
<details>
<summary>Proposed Implementation of `request`</summary>

```rust,ignore
{{#include ../../../rust-runtime/aws-smithy-runtime-api/src/http/request.rs}}
```
</details>

### Future Work
Currently, the only way to construct `Request` is from a compatible type (e.g. `http02x::Request`)

Changes checklist
-----------------
- [x] Implement initial implementation and test it against the SDK as written
- [ ] Add test suite of `HTTP` wrapper
- [ ] External design review
- [x] Update the SigV4 crate to remove `http` API dependency
- [ ] Update the SDK to use the new type (breaking change)
