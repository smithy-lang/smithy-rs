<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Eliminating Public `http` dependencies
=============

> Status: Accepted
>
> Applies to: client

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines how we plan to refactor the SDK to allow the SDK to consume a `1.0` version of `hyper`, `http-body`,
and `http` at a later date. Currently, `hyper` is `0.14.x` and a `1.0` release candidate series is in progress. However,
there are [open questions](https://github.com/orgs/hyperium/projects/1/views/4) that may significantly delay the launch
of
these three crates. We do not want to tie the `1.0` of the Rust SDK to these crates.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **http-body**: A crate (and trait) defining how HTTP bodies work. Notably, the change from `0.*` to `1.0` changes `http-body`
  to operate on frames instead of having separate methods.
- `http` (crate): a low level crate of `http` primitives (no logic, just requests and responses)
- ossified dependency: An ossified dependency describes a dependency that, when a new version is released, cannot be utilized without breaking changes. For example, if the `mutate_request` function on every operation operates on `&mut http::Request` where `http = 0.2`, that dependency is "ossified." Compare this to a function that offers the ability to convert something into an `http = 0.2` request—since http=1 and http=0.2 are largely equivalent, the
  existence of this function does not prevent us from using http = 1 in the future. In general terms, **functions that operate on references are much more likely to ossify**—There is no practical way for someone to mutate an `http = 0.2` request if you have an `http = 1` request other than a time-consuming clone, and reconversion process.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->

## Why is this important?

**Performance**:
At some point in the Future, `hyper = 1`, `http = 1` and `http-body = 1` will be released. It takes ~1-2 microseconds to rebuild an HTTP request. If we assume that `hyper = 1` will only operate on `http = 1` requests, then if we can't use `http = 1` requests internally, our only way of supporting `hyper = 1` will be to convert the HTTP request at dispatch time. Besides pinning us to a potentially unsupported version of the HTTP crate, this will prevent us from directly dispatching requests in an efficient manner. With a total overhead of 20µs for the SDK, 1µs is not insignificant. Furthermore, it grows as the number of request headers grow. A benchmark should be run for a realistic HTTP request e.g. one that we send to S3.

**Hyper Upgrade**:
Hyper 1 is significantly more flexible than Hyper 0.14.x, especially WRT to connection management & pooling. If we don't make these changes, the upgrade to Hyper 1.x could be significantly more challenging.

**Security Fixes**:
If we're still on `http = 0.*` and a vulnerability is identified, we may end up needing to manually contribute the patch. The `http` crate is not trivial and contains parsing logic and optimized code (including a non-trivial amount of `unsafe`). [See this GitHub issue](https://github.com/hyperium/http/issues/412). Notable is that one issue may be unsound and result in changing the public API.

**API Friendliness**
If we ship with an API that public exposes customers to `http = 0.*`, we have the API forever. We have to consider that we aren't shipping the Rust SDK for this month or even this year but probably the Rust SDK for the next 5-10 years.

**Future CRT Usage**
If we make this change, we enable a future where we can use the CRT HTTP request type natively without needing a last minute conversion to the CRT HTTP Request type.
```rust,ignore
struct HttpRequest {
  inner: Inner
}

enum Inner {
  Httpv0(http_0::Request),
  Httpv1(http_1::Request),
  Crt(aws_crt_http::Request)
}
```

The user experience if this RFC is implemented
----------------------------------------------
Customers are impacted in 3 main locations:
1. HTTP types in Interceptors
2. HTTP types in `customize(...)`
3. HTTP types in Connectors

In all three of these cases, users would interact with our `http` wrapper types instead.


In the current version of the SDK, we expose public dependencies on the `http` crate in several key places:

1. The `sigv4` crate. The `sigv4` crate currently operates directly on many types from the `http` crate. This is unnecessary and actually makes the crate more difficult to use. Although `http` may be used internally, `http` will be removed from the public API of this crate.
2. Interceptor Context: `interceptor`s can mutate the HTTP request through an unshielded interface. This requires creating a [wrapper layer around `http::Request`](#http-request-wrapper) and updating already written interceptors.
3. `aws-config`: `http::Response` and `uri`
4. A long tail of exposed requests and responses in the runtime crates. Many of these crates will be removed post-orchestrator so this can be temporarily delayed.

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

### Enabling API evolution
One key mechanism that we SHOULD use for allowing our APIs to evolve in the future is usage of `~` version bounds for the runtime crates after releasing 1.0.

### Http Request Wrapper

In order to enable HTTP evolution, we will create a set of wrapper structures around `http::Request` and `http::Response`. These will use `http = 0` internally. Since the HTTP crate itself is quite small, including private dependencies on both versions of the crate is a workable solution. In general, we will aim for an API that is close to drop-in compatible to the HTTP crate while ensuring that a different crate could be used as the backing storage.

```rust,ignore
// since it's our type, we can default `SdkBody`
pub struct Request<B = SdkBody> {
    // this uses the http = 0.2 request. In the future, we can make an internal enum to allow storing an http = 1
    http_0: http::Request<B>
}
```

**Conversion to/from `http::Request`**
One key property here is that although converting to/from an `http::Request` **can** be expensive, this is *not* ossification of the API. This is because the API can support converting from/to both `http = 0` and `http = 1` in the future—because it offers mutation of the request via a unified interface, the request would only need to be converted once for dispatch if there was a mismatch (instead of repeatedly). At some point in the future, the `http = 0` representation could be deprecated and removed or feature gated.

**Challenges**
1. Creating an HTTP API which is forwards compatible, idiomatic and "truthful" without relying on existing types from Hyper—e.g. when adding a header, we need to account for the possibility that a header is invalid.
2. Allow for future forwards-compatible evolution in the API—A lot of thought went into the `http` crate API w.r.t method parameters, types, and generics. Although we can aim for a simpler solution in some cases (e.g. accepting `&str` instead of `HeaderName`), we need to be careful that we do so while allowing API evolution.

### Removing the SigV4 HTTP dependency
The SigV4 crate signs a number of `HTTP` types directly. We should change it to accept strings, and when appropriate, iterators of strings for headers.

### Removing the HTTP dependency from generated clients
Generated clients currently include a public HTTP dependency in `customize`. This should be changed to accept our `HTTP` wrapper type instead or be restricted to a subset of operations (e.g. `add_header`) while forcing users to add an interceptor if they need full control.


<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [ ] Create the `http::Request` wrapper. Carefully audit for compatibility without breaking changes. 5 Days.
- [ ] Refactor currently written interceptors to use the wrapper: 2 days.
- [ ] Refactor the SigV4 crate to remove the HTTP dependency from the public interface: 2 days.
- [ ] Add / validate support for SdkBody `http-body = 1.0rc.2` either in a PR or behind a feature gate. Test this to
  ensure it works with Hyper. Some previous work here exists: 1 week
- [ ] Remove `http::Response` and `Uri` from the public exposed types in `aws-config`: 1-4 days.
- [ ] Long tail of other usages: 1 week
- [ ] Implement `~` versions for SDK Crate => runtime crate dependencies: 1 week
