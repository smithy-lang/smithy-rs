RFC: Callback APIs for `ByteStream` and `SdkBody`
=================================================

> Status: RFC

Adding a callback API to `ByteStream` and `SdkBody` will enable developers using the SDK to implement things like checksum validations and 'read progress' callbacks.

## The Implementation

*Note that comments starting with '//' are not necessarily going to be included in the actual implementation and are intended as clarifying comments for the purposes of this RFC.*

```rust,ignore
// in aws_smithy_http::callbacks...

/// A callback that, when inserted into a request body, will be called for corresponding lifecycle events.
trait BodyCallback: Send {
   /// This lifecycle function is called for each chunk **successfully** read. If an error occurs while reading a chunk,
   /// this method will not be called. This method takes `&mut self` so that implementors may modify an implementing
   /// struct/enum's internal state. Implementors may return an error.
   fn update(&mut self, #[allow(unused_variables)] bytes: &[u8]) -> Result<(), BoxError> { Ok(()) }

   /// This callback is called once all chunks have been read. If the callback encountered one or more errors
   /// while running `update`s, this is how those errors are raised. Implementors may return a [`HeaderMap`][HeaderMap]
   /// that will be appended to the HTTP body as a trailer. This is only useful to do for streaming requests.
   fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> { Ok(None) }

   /// Create a new `BodyCallback` from an existing one. This is called when a `BodyCallback` needs to be
   /// re-initialized with default state. For example: when a request has a body that needs to be
   /// rebuilt, all callbacks for that body need to be run again but with a fresh internal state.
   fn make_new(&self) -> Box<dyn BodyCallback>;
}

impl BodyCallback for Box<dyn BodyCallback> {
   fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> { BodyCallback::update(self, bytes) }
   fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> { BodyCallback::trailers(self) }
   fn make_new(&self) -> Box<dyn SendCallback> { BodyCallback::make_new(self) }
}
```

The changes we need to make to `ByteStream`:

*(The current version of `ByteStream` and `Inner` can be seen [here][ByteStream impls].)*

```rust,ignore
// in `aws_smithy_http::byte_stream`...

// We add a new method to `ByteStream` for inserting callbacks
impl ByteStream {
    // ...other impls omitted

    // A "builder-style" method for setting callbacks
    pub fn with_body_callback(&mut self, body_callback: Box<dyn BodyCallback>) -> &mut Self {
        self.inner.with_body_callback(body_callback);
        self
    }
}

impl Inner<SdkBody> {
    // `Inner` wraps an `SdkBody` which has a "builder-style" function for adding callbacks.
    pub fn with_body_callback(&mut self, body_callback: Box<dyn BodyCallback>) -> &mut Self {
        self.body.with_body_callback(body_callback);
        self
    }
}
```

The changes we need to make to `SdkBody`:

*(The current version of `SdkBody` can be seen [here][SdkBody impls].)*

```rust,ignore
// In aws_smithy_http::body...

#[pin_project]
pub struct SdkBody {
    #[pin]
    inner: Inner,
    rebuild: Option<Arc<dyn (Fn() -> Inner) + Send + Sync>>,
    // We add a `Vec` to store the callbacks
    #[pin]
    callbacks: Vec<Box<dyn BodyCallback>>,
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
               for callback in this.callbacks.iter_mut() {
                  // Callbacks can run into errors when reading bytes. They'll be surfaced here
                  callback.update(bytes)?;
               }
            }
            // When we're done polling for bytes, run each callback's `trailers()` method. If any calls to
            // `trailers()` return an error, propagate that error up. Otherwise, continue.
            Poll::Ready(None) => {
                for callback_result in this.callbacks.iter().map(BodyCallback::trailers) {
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
            let callbacks = self
                .callbacks
                .iter()
                .map(Callback::make_new)
                .collect();

            Self {
                inner: next,
                rebuild: self.rebuild.clone(),
                callbacks,
            }
        })
    }

    pub fn with_callback(&mut self, callback: BodyCallback) -> &mut Self {
        self.callbacks.push(callback);
        self
    }
}

/// Given two [`HeaderMap`][HeaderMap]s, merge them together and return the merged `HeaderMap`. If the
/// two `HeaderMap`s share any keys, values from the right `HeaderMap` be appended to the left `HeaderMap`.
///
/// # Example
///
/// ```rust
/// let header_name = HeaderName::from_static("some_key");
///
/// let mut left_hand_side_headers = HeaderMap::new();
/// left_hand_side_headers.insert(
///     header_name.clone(),
///     HeaderValue::from_str("lhs value").unwrap(),
/// );
///
/// let mut right_hand_side_headers = HeaderMap::new();
/// right_hand_side_headers.insert(
///     header_name.clone(),
///     HeaderValue::from_str("rhs value").unwrap(),
/// );
///
/// let merged_header_map =
///     append_merge_header_maps(left_hand_side_headers, right_hand_side_headers);
/// let merged_values: Vec<_> = merged_header_map
///     .get_all(header_name.clone())
///     .into_iter()
///     .collect();
///
/// // Will print 'some_key: ["lhs value", "rhs value"]'
/// println!("{}: {:?}", header_name.as_str(), merged_values);
/// ```
fn append_merge_header_maps(
    mut lhs: HeaderMap<HeaderValue>,
    rhs: HeaderMap<HeaderValue>,
) -> HeaderMap<HeaderValue> {
    let mut last_header_name_seen = None;
    for (header_name, header_value) in rhs.into_iter() {
        // For each yielded item that has None provided for the `HeaderName`,
        // then the associated header name is the same as that of the previously
        // yielded item. The first yielded item will have `HeaderName` set.
        // https://docs.rs/http/latest/http/header/struct.HeaderMap.html#method.into_iter-2
        match (&mut last_header_name_seen, header_name) {
            (_, Some(header_name)) => {
                lhs.append(header_name.clone(), header_value);
                last_header_name_seen = Some(header_name);
            }
            (Some(header_name), None) => {
                lhs.append(header_name.clone(), header_value);
            }
            (None, None) => unreachable!(),
        };
    }

    lhs
}

impl http_body::Body for SdkBody {
    // The other methods have been omitted because they haven't changed

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        let header_map = self
            .callbacks
            .iter()
            .filter_map(|callback| {
                match callback.trailers() {
                    Ok(optional_header_map) => optional_header_map,
                    // early return if a callback encountered an error
                    Err(e) => { return e },
                }
            })
            // Merge any `HeaderMap`s from the last step together, one by one.
            .reduce(append_merge_header_maps);

        Poll::Ready(Ok(header_map))
    }
}
```

## Implementing Checksums

What follows is a simplified example of how this API could be used to introduce checksum validation for outgoing request payloads. In this example, the checksum calculation is fallible and no validation takes place. All it does it calculate
the checksum of some data and then returns the checksum of that data when `trailers` is called. This is fine because it's
being used to calculate the checksum of a streaming body for a request.

```rust,ignore
#[derive(Default)]
struct Crc32cChecksumCallback {
    state: Option<u32>,
}

impl ReadCallback for Crc32cChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.state = match self.state {
            Some(crc) => { self.state = Some(crc32c_append(crc, bytes)) }
            None => { Some(crc32c(&bytes)) }
        };

       Ok(())
    }

    fn trailers(&self) ->
    Result<Option<HeaderMap<HeaderValue>>,
          Box<dyn std::error::Error + Send + Sync>>
    {
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

*NOTE: If `Crc32cChecksumCallback` needed to validate a response, then we could modify it to check its internal state against a target checksum value and calling `trailers` would produce an error if the values didn't match.*

In order to use this in a request, we'd modify codegen for that request's service.

1. We'd check if the user had requested validation and also check if they'd pre-calculated a checksum.
2. If validation was requested but no pre-calculated checksum was given, we'd create a callback similar to the one above
3. Then, we'd create a new checksum callback and:
   - (if streaming) we'd set the checksum callback on the request body object
   - (if non-streaming) we'd immediately read the body and call `BodyCallback::update` manually. Once all data was read, we'd get the checksum by calling `trailers` and insert that data as a request header.

[ByteStream impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/byte_stream.rs#L205
[SdkBody impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/body.rs#L71
