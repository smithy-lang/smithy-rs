RFC: Callback APIs for `ByteStream` and `SdkBody`
=================================================

> Status: RFC

Adding a callback APIs to `ByteStream` and `SdkBody` will enable developers using the SDK to implement things like checksum validations and 'read progress' callbacks.

## The Implementation

*Note that comments starting with '//' are not necessarily going to be included in the actual implementation and are intended as clarifying comments for the purposes of this RFC.*

```rust
// in aws_smithy_http::read_callback...

// Each trait method defaults to doing nothing. It's up to implementors to
// implement one or both of the trait methods
/// Structs and enums implementing this trait can be inserted into a `ByteStream`,
/// and will then be called in reaction to various events during a `ByteStream`'s
/// lifecycle.
pub trait ReadCallback: Send + Sync {
    /// This callback is called for each chunk **successfully** read.
    /// If an error occurs while reading a chunk, this will not be called.
    /// This function takes `&mut self` so that implementors may modify
    /// an implementing struct/enum's internal state.
    // In order to stop the compiler complaining about these empty default impls,
    // we allow unused variables.
    fn update(&mut self, #[allow(unused_variables)] bytes: &[u8]) {}

    /// This callback is called once all chunks have been read. If the callback encountered 1 or more errors
    /// while running `update`s, this is how those errors are raised.
    fn finally(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> { Ok(()) }

    /// return any trailers to be appended to this `ByteStream` if it's used to
    /// create the body of an HTTP request.
    // `HeaderMap`/`HeaderValue` are defined by `hyper`
    fn trailers(&self) -> Option<HeaderMap<HeaderValue>> { None }

    /// Create a new `ReadCallback` from an existing one. This is called when a `ReadCallback` need
    /// to be re-initialized with default state. For example: when a request has a body that needs
    /// to be rebuilt, all read callbacks on that body need to be run again but with a fresh internal state.
    fn make_new(&self) -> Box<dyn ReadCallback>;
}

// We also impl `ReadCallback` for `Box<dyn ReadCallback>` because it makes callback trait objects easier to work with.
```

The changes we need to make to `ByteStream`:

*(The current version of `ByteStream` and `Inner` can be seen [here][ByteStream impls].)*

```rust
// in `aws_smithy_http::byte_stream`...

// We add a new method to `ByteStream` for inserting callbacks
impl ByteStream {
    // ...other impls omitted

    // A "builder-style" method for setting callbacks
    pub fn with_callback(&mut self, callback: Box<dyn ReadCallback>) -> &mut Self {
        self.inner.with_callback(callback);
        self
    }
}

impl Inner<SdkBody> {
    // `Inner` wraps an `SdkBody` which has a "builder-style" function for adding callbacks.
    pub fn with_read_callback(&mut self, read_callback: Box<dyn ReadCallback>) -> &mut Self {
        self.body.with_read_callback(read_callback);
        self
    }
}
```

The changes we need to make to `SdkBody`:

*(The current version of `SdkBody` can be seen [here][SdkBody impls].)*

```rust
// In aws_smithy_http::body...

#[pin_project]
pub struct SdkBody {
    #[pin]
    inner: Inner,
    rebuild: Option<Arc<dyn (Fn() -> Inner) + Send + Sync>>,
    // We add a `Vec` to store the callbacks
    #[pin]
    read_callbacks: Vec<Box<dyn ReadCallback>>,
}

impl SdkBody {
    // We update the various fns that create `SdkBody`s to create an empty `Vec` to store callbacks.
    // Those updates are very simple so I've omitted them from this code example.

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, Error>>> {
        let mut this = self.project();
        // This block is old. I've included for context.
        let polling_result = match this.inner.project() {
            InnerProj::Once(ref mut opt) => {
                let data = opt.take();
                match data {
                    Some(bytes) if bytes.is_empty() => Poll::Ready(None),
                    Some(bytes) => Poll::Ready(Some(Ok(bytes))),
                    None => Poll::Ready(None),
                }
            }
            InnerProj::Streaming(body) => body.poll_data(cx).map_err(|e| e.into()),
            InnerProj::Dyn(box_body) => box_body.poll_data(cx),
            InnerProj::Taken => {
                Poll::Ready(Some(Err("A `Taken` body should never be polled".into())))
            }
        };

        // This block is new.
        match &polling_result {
            // When we get some bytes back from polling, pass those bytes to each callback in turn
            Poll::Ready(Some(Ok(bytes))) => {
                this.read_callbacks
                    .iter_mut()
                    .for_each(|callback| callback.update(bytes));
            }
            // When we're done polling for bytes, run each callback's `finally()` method. If any calls to
            // `finally()` return an error, propagate that error up. Otherwise, continue.
            Poll::Ready(None) => {
                for callback_result in this.read_callbacks.iter().map(ReadCallback::finally) {
                    if let Err(e) = callback_result {
                        return Poll::Ready(Some(Err(e)));
                    }
                }
            }
            _ => (),
        }

        // Now that we've inspected the polling result, all that's left to do is to return it.
        polling_result
    }

    // This function now has the added responsibility of cloning callback functions (but with fresh state)
    // in the case that the `SdkBody` needs to be rebuilt.
    pub fn try_clone(&self) -> Option<Self> {
        self.rebuild.as_ref().map(|rebuild| {
            let next = rebuild();
            let read_callbacks = self
                .read_callbacks
                .iter()
                .map(ReadCallback::make_new)
                .collect();

            Self {
                inner: next,
                rebuild: self.rebuild.clone(),
                read_callbacks,
            }
        })
    }

    pub fn with_read_callback(&mut self, read_callback: Box<dyn ReadCallback>) -> &mut Self {
        self.read_callbacks.push(read_callback);
        self
    }
}

impl http_body::Body for SdkBody {
    // The other methods have been omitted because they haven't changed

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        let mut last_header_key_seen = None;
        let header_map = self
            .read_callbacks
            .iter()
            .filter_map(|callback| callback.trailers())
            .reduce(|mut left_header_map, mut right_header_map| {
                right_header_map.into_iter().for_each(|(key, value)| {
                    // For each yielded item that has None provided for the `HeaderName`,
                    // then the associated header name is the same as that of the previously
                    // yielded item. The first yielded item will have `HeaderName` set.
                    // https://docs.rs/http/latest/http/header/struct.HeaderMap.html#method.into_iter-2
                    match (last_header_key_seen, key) {
                        (_, Some(key)) => {
                            left_header_map.append(key, value);
                            last_header_key_seen = Some(key);
                        }
                        (Some(key), None) => {
                            left_header_map.append(key, value);
                        }
                        (None, None) => unreachable!(),
                    };
                });

                left_header_map
            });

        Poll::Ready(Ok(header_map))
    }
}
```

## Implementing Checksums

What follows is a simplified example of how this API could be used to introduce checksum validation for outgoing request payloads. In this example, the checksum calculation is fallible and no validation takes place. All it does it calculate
the checksum of some data and then returns the checksum of that data when `trailers` is called. This is fine because it's
being used to calculate the checksum of a streaming body in a request.

```rust
#[derive(Default)]
struct Crc32cChecksumCallback {
    state: Option<u32>,
}

impl ReadCallback for Crc32cChecksumCallback {
    fn update(&mut self, bytes: &[u8]) {
        self.state = match self.state {
            Some(crc) => { self.state = Some(crc32c_append(crc, bytes)) }
            None => { Some(crc32c(&bytes)) }
        };
    }

    fn trailers(&self) -> Option<HeaderMap<HeaderValue>> {
        let mut header_map = HeaderMap::new();
        // This checksum name is an Amazon standard and would be a `const` in the real implementation
        let key = HeaderName::from_static("x-amz-checksum-crc32c");
        // If no data was provided to this callback and no CRC was ever calculated, we return zero as the checksum.
        let crc = self.state.unwrap_or_default();
        // Convert the CRC to a string, base 64 encode it, and then convert it into a `HeaderValue`.
        let value = HeaderValue::from_str(&base64::encode(crc.to_string())).expect("base64 will always produce valid header values");

        header_map.insert(key, value);

        Some(header_map)
    }

    fn make_new(&self) -> Box<dyn ReadCallback> {
        Box::new(Crc32cChecksumCallback::default())
    }
}
```

*NOTE: If `Crc32cChecksumCallback` needed to validate a response, then we could modify it to check its internal state against a target checksum value and calling `finally` would produce an error if the values didn't match.*

In order to use this in a request, we'd modify codegen for that request's service.

1. We'd check if the user had requested validation and also check if they'd pre-calculated a checksum.
2. If validation was requested but no pre-calculated checksum was given, we'd create a callback similar to the one above
3. Then, we'd create a new checksum callback and:
   - (if streaming) we'd set the checksum callback on the request body object
   - (if non-streaming) we'd immediately read the body and call `ReadCallback::update` manually. Once all data was read, we'd get the checksum by calling `trailers` and insert that data as a request header.

## Other thoughts

- What if we defined a `headers` method on `ReadCallback` too? We could just have it default to calling `trailers` internally by default (or vice versa.) This would make it less confusing when we manually call the checksum callback in order to set headers.

[ByteStream impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/byte_stream.rs#L205
[SdkBody impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/body.rs#L71
