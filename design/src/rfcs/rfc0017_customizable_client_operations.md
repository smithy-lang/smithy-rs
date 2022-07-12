RFC: Customizable Client Operations
===================================

> Status: RFC

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

SDK customers occasionally need to add additional HTTP headers to requests, and currently,
the SDK has no easy way to accomplish this. At time of writing, the lower level Smithy
client has to be used to create an operation, and then the HTTP request augmented on
that operation type. For example:

```rust
let input = SomeOperationInput::builder().some_value(5).build()?;

let operation = {
    let op = input.make_operation(&service_config).await?;
    let (request, response) = op.into_request_response();

    let request = request.augment(|req, _props| {
        req.headers_mut().insert(
            HeaderName::from_static("x-some-header"),
            HeaderValue::from_static("some-value")
        );
        Result::<_, Infallible>::Ok(req)
    })?;

    Operation::from_parts(request, response)
};

let response = smithy_client.call(operation).await?;
```

This approach is both difficult to discover and implement since it requires acquiring
a Smithy client rather than the generated fluent client, and it's anything but ergonomic.

This RFC proposes an easier way to augment requests that is compatible with the fluent
client.

Terminology
-----------

- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy.
- **Fluent Client**: A code generated `Client` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.

Proposal
--------

The code generated fluent builders returned by the fluent client should have a method added to them,
similar to `send`, but that returns a customizable request. The customer experience should look as
follows:

```rust
let response = client.some_operation()
    .some_value(5)
    .customize()?
    .map_request(|mut req| {
        req.headers_mut().insert(
            HeaderName::from_static("x-some-header"),
            HeaderValue::from_static("some-value")
        );
        Result::<_, Infallible>::Ok(req)
    })?
    .send()
    .await?;
```

This new `customize` method would return the following:

```rust
pub struct CustomizableOperation<O, R> {
    handle: Arc<Handle>,
    operation: Operation<O, R>,
}

impl<O, R> CustomizableOperation<O, R> {
    // Allows for customizing the operation's request
    fn map_request<E>(
        mut self,
        f: impl FnOnce(Request<SdkBody>) -> Result<Request<SdkBody>, E>,
    ) -> Result<Self, E> {
        let (request, response) = self.operation.into_request_response();
        let request = request.augment(|req, _props| f(req))?;
        self.operation = Operation::from_parts(request, response);
        Ok(self)
    }

    // Allows for customizing the entire operation
    fn map_operation<E>(
        mut self,
        f: impl FnOnce(Operation<O, R>) -> Result<Operation<O, R>, E>,
    ) -> Result<Self, E> {
        self.operation = f(self.operation)?;
        Ok(self)
    }

    // Direct access to read the request
    fn request(&self) -> &Request<SdkBody> {
        self.operation.request()
    }

    // Direct access to mutate the request
    fn request_mut(&mut self) -> &mut Request<SdkBody> {
        self.operation.request_mut()
    }

    // Sends the operation's request
    async fn send<T, E>(self) -> Result<T, SdkError<E>>
    where
        O: ParseHttpResponse<Output = Result<T, E>> + Send + Sync + Clone + 'static,
        E: std::error::Error,
        R: ClassifyResponse<SdkSuccess<T>, SdkError<E>> + Send + Sync,
    {
        self.handle.client.call(self.operation).await
    }
}
```

Additionally, for those who want to avoid closures, the `Operation` type will have
`request` and `request_mut` methods added to it to get direct access to its underlying
HTTP request.

The `CustomizableOperation` type will then mirror these functions so that the experience
can look as follows:

```rust
let mut operation = client.some_operation()
    .some_value(5)
    .customize()?;
operation.request_mut()
    .headers_mut()
    .insert(
        HeaderName::from_static("x-some-header"),
        HeaderValue::from_static("some-value")
    );
let response = operation.send().await?;
```

### Eliminating `async` from `make_operation`

To avoid requiring an `await` after the call to `customize`, the `make_operation` function
generated on input structs needs to be made synchronous. The `make_operation` function
originally was synchronous, but was made async during the implementation of the Glacier
customizations (#797, #801, #1474). It should be possible to move this use case into
middleware to make it sync again.

### Why the name `customize`?

Alternatively, the name `build` could be used, but this increases the odds that
customers won't realize that they can call `send` directly, and then call a longer
`build`/`send` chain when customization isn't needed:

```rust
client.some_operation()
    .some_value()
    .build() // Oops, didn't need to do this
    .send()
    .await?;
```

vs.

```rust
client.some_operation()
    .some_value()
    .send()
    .await?;
```

Additionally, no AWS services at time of writing have a member named `customize`
that would conflict with the new function, so adding it would not be a breaking change.

Changes Checklist
-----------------

- [ ] Refactor `make_operation` to no longer be `async` so that the `customize` function doesn't need to be followed by `await`.
      The asynchronous parts of `make_operation` will need to become middleware.
- [ ] Create `CustomizableOperation` as an inlinable, and code generate it into `client` so that it has access to `Handle`
- [ ] Code generate the `customize` method on fluent builders
- [ ] Update the `RustReservedWords` class to include `customize`
- [ ] Add ability to mutate the HTTP request on `Operation`
- [ ] Add examples for both approaches
- [ ] Comment on older discussions asking about how to do this with this improved approach
