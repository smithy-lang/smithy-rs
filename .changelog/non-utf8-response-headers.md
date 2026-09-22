---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["aajtodd"]
references: []
breaking: false
new_feature: true
bug_fix: true
---
A response header value that is not valid UTF-8 no longer fails the entire response.

HTTP permits any octet except a control character in a header value, so a service may legitimately
echo back bytes that are not valid UTF-8 — for example a customer-supplied string stored in another
encoding. Previously `Headers` rejected the whole header map if any single value was not valid
UTF-8. That happened during the HTTP-response-to-`Headers` conversion, before deserialization and
regardless of whether anything read that header, and it surfaced as a non-retryable
`DispatchFailure` with no way to inspect the response.

Header values are now stored exactly as received, and the encoding requirement applies where a
value is actually consumed:

- **A header bound to no modeled member is now harmless.** Previously any non-UTF-8 header in the
  map failed the operation.
- **A header bound to a modeled member reports an error naming that member**, rather than being
  silently discarded: `Error::unhandled` identifying the member and header name on the client, and
  a 400 on the server. Nothing is dropped without saying so.

### Reading a value that is not UTF-8

`Headers` gained byte accessors alongside the existing string ones. The string accessors
(`get`, `get_all`, `iter`, `remove`) yield only values that are valid UTF-8 and skip the rest;
the new byte accessors return every value:

- `Headers::get_bytes`, `Headers::get_all_bytes`, `Headers::iter_bytes`
- `HeaderValue::as_bytes`, `HeaderValue::try_as_str`

Note that `Headers::len` and `Headers::contains_key` count and report values the string accessors
skip.

Values are stored as received, so converting a `Headers` back out to an `http::HeaderMap`
reproduces the original octets rather than a re-encoded string.

The requirement on *writes* is unchanged: `insert` still panics and `try_insert` still returns an
error for a value that is not valid UTF-8, so no `HeaderValue` a caller can name is non-UTF-8 and
`HeaderValue::as_str` still cannot panic.

### Recovering a value and letting the operation succeed

A caller that does not need the value can capture the raw octets and drop the header before
deserialization, after which the member deserializes to `None` and the operation succeeds. This
needs no configuration support:

```rust
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeDeserializationInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
struct NonUtf8Headers {
    captured: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl Intercept for NonUtf8Headers {
    fn name(&self) -> &'static str {
        "NonUtf8Headers"
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let headers = context.response_mut().headers_mut();
        let captured: Vec<(String, Vec<u8>)> = headers
            .iter_bytes()
            .filter(|(_, value)| std::str::from_utf8(value).is_err())
            .map(|(name, value)| (name.to_owned(), value.to_vec()))
            .collect();
        for (name, _) in &captured {
            headers.remove(name);
        }
        // Overwrite rather than append, so this describes the response about to be deserialized
        // even after a retry.
        *self.captured.lock().unwrap() = captured;
        Ok(())
    }
}
```

Register it on the client config, or per operation via `.customize().interceptor(..)`, and read the
captured octets after `send()` returns. Note that `Headers::remove` removes every value for a
header name, so for a multi-value header this also drops the values that *were* valid UTF-8;
re-insert them if you need them.
