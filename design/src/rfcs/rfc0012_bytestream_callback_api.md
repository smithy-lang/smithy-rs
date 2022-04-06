RFC: A callback API for `ByteStream`
====================================

> Status: RFC

Adding a callback API to `ByteStream` will enable developers using the SDK to implement things like checksum validations and 'read progress' callbacks.

## The Implementation

*Note that comments starting with '//' are not necessarily going to be included in the actual implementation and are intended as clarifying comments for the purposes of this RFC.*

```rust
// Each trait method defaults to doing nothing. It's up to implementors to
// implement one or both of the trait methods
/// Structs and enums implementing this trait can be inserted into a `ByteStream`,
/// and will then be called in reaction to various events during a `ByteStream`'s
/// lifecycle.
pub trait ByteStreamReadCallback
// I don't really like this bound but we need it because this will be inserted into an `Inner`
where
    Self: Debug,
{
    /// This callback is called for each chunk **successfully** read.
    /// If an error occurs while reading a chunk, this will not be called.
    /// This function takes `&mut self` so that implementors may modify
    /// an implementing struct/enum's internal state.
    // In order to stop the compiler complaining about these empty default impls,
    // we allow unused variables.
    fn on_read_chunk(&mut self, #[allow(unused_variables)] chunk: &Bytes) {}

    /// This callback is called once all chunks have been successfully read.
    /// It's passed a reference to the chunks in the form of an `AggregatedBytes`.
    /// This function takes `&mut self` so that implementors may modify an
    /// implementing struct/enum's internal state.
    fn finally(&mut self, #[allow(unused_variables)] aggregated_bytes: &AggregatedBytes) {}

    /// return any trailers to be appended to this `ByteStream` if it's used to
    /// create the body of an HTTP request.
    // `HeaderMap`/`HeaderValue` are defined by `hyper`
    fn trailers(&self) -> Option<HeaderMap<HeaderValue>> {}
}

// We add a new method to `ByteStream` for inserting callbacks
impl ByteStream {
    // ...other impls omitted

    // A "builder-style" method for setting callbacks
    pub fn with_callback(&mut self, callback: Arc<dyn ByteStreamReadCallback>) -> &mut Self {
        self.inner.with_callback(callback);
        self
    }
}

// Callbacks actually get stored in the `Inner` struct because that's where
// the chunk-reading actually occurs.
#[pin_project]
#[derive(Debug, Clone)]
struct Inner<B> {
    #[pin]
    body: B,
    // This field is new. It's going to store callbacks that get called when we're
    // reading the `SdkBody` chunk-by-chunk. This field doesn't get checked for
    // equality purposes.
    callbacks: Vec<Arc<dyn ByteStreamReadCallback>>
}

impl PartialEq for Inner<B: PartialEq> {
    fn eq(&self, rhs: &Self) -> bool { self.body == rhs.body }
}

impl Eq for Inner<B: Eq> {}

impl<B> Inner<B> {
    // ...other impls omitted

    pub fn new(body: B) -> Self {
        Self { body, callbacks: Vec::new() }
    }

    // `Inner` also gets a "builder-style" function for adding callbacks
    pub fn with_callback(&mut self, callback: Arc<dyn ByteStreamReadCallback>) -> &mut self {
        self.callbacks.push(callback);
        self
    }

    pub async fn collect(self) -> Result<AggregatedBytes, B::Error>
        where
            B: http_body::Body<Data = Bytes>,
    {
        let mut output = SegmentedBuf::new();
        let body = self.body;
        crate::pin_mut!(body);
        while let Some(buf) = body.data().await {
            // If we successfully read some bytes,
            // then we call each callback in turn,
            // passing a ref to those bytes.
            if let Ok(bytes) = buf.as_ref() {
                self.callbacks.iter_mut().for_each(|callback| {
                    callback.on_read_chunk(bytes);
                })
            }
            output.push(buf?);
        }

        let aggregated_bytes = AggregatedBytes(output);

        // We also call the callback at the end too.
        self.callbacks.iter_mut().for_each(|callback| {
            callback.finally(&aggregated_bytes)
        });

        Ok(aggregated_bytes)
    }
}
```

The current version of `ByteStream` and `Inner` can be seen [here][ByteStream impls].

[ByteStream impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/byte_stream.rs#L205
