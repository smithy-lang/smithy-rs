---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["aajtodd"]
references: []
breaking: false
new_feature: true
bug_fix: true
---
A response header value that is not valid UTF-8 no longer fails the whole response.

An HTTP header value may contain any octet except a control character, so a service can send a value
that is not representable as a Rust `String`. Previously one such value failed the entire response
during the HTTP-to-SDK conversion — before deserialization, whether or not anything read that header,
and surfacing as a non-retryable `DispatchFailure`.

Header values are now stored as received, and the encoding requirement applies where a value is bound
to a modeled member. A header bound to no member is harmless; a header bound to a member reports an
error naming that member on the client, or a 400 on the server. Nothing is dropped silently.

`Headers` gained byte accessors that return every value, alongside the existing string accessors,
which now skip values that are not valid UTF-8:

- `Headers::get_bytes`, `Headers::get_all_bytes`, `Headers::iter_bytes`
- `HeaderValue::as_bytes`, `HeaderValue::try_as_str`

Note that `Headers::len` and `Headers::contains_key` count and report values the string accessors
skip.

To tolerate an unreadable value rather than fail, put `NonUtf8HeaderHandling::Skip` in the config bag
from an interceptor. The member then deserializes as if the header were absent, and because the
header is left in place the octets stay readable — so a caller that needs the value can decode it
however its service encodes it:

```rust
fn read_before_execution(
    &self,
    _context: &BeforeSerializationInterceptorContextRef<'_>,
    cfg: &mut ConfigBag,
) -> Result<(), BoxError> {
    cfg.interceptor_state().store_put(NonUtf8HeaderHandling::Skip);
    Ok(())
}

fn read_after_deserialization(
    &self,
    context: &AfterDeserializationInterceptorContextRef<'_>,
    _runtime_components: &RuntimeComponents,
    _cfg: &mut ConfigBag,
) -> Result<(), BoxError> {
    if let Some(value) = context.response().headers().get_bytes("x-amz-expiration") {
        // ISO-8859-1: every octet is one code point, so this cannot fail.
        let decoded: String = value.iter().map(|&b| b as char).collect();
        *self.expiration.lock().unwrap() = Some(decoded);
    }
    Ok(())
}
```

`Skip` always yields `None` for the whole member, never a partial value — including for
`@httpPrefixHeaders`, where one unreadable entry makes the whole map `None` rather than a map
silently missing that entry.

Register a capturing interceptor **per operation** (`.customize().interceptor(..)`) rather than on the
client: one registered on the client shares a single handle across every request, so captured values
cannot be attributed to a particular call. Setting `Skip` itself is stateless and is fine to do
client-wide.
