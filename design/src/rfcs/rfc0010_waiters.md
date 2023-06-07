RFC: Waiters
============

> Status: Accepted

Waiters are a convenient polling mechanism to wait for a resource to become available or to
be deleted. For example, a waiter could be used to wait for a S3 bucket to be created after
a call to the `CreateBucket` API, and this would only require a small amount of code rather
than building out an entire polling mechanism manually.

At the highest level, a waiter is a simple polling loop (pseudo-Rust):

```rust,ignore
// Track state that contains the number of attempts made and the previous delay
let mut state = initial_state();

loop {
    // Poll the service
    let result = poll_service().await;

    // Classify the action that needs to be taken based on the Smithy model
    match classify(result) {
        // If max attempts hasn't been exceeded, then retry after a delay. Otherwise, error.
        Retry => if state.should_retry() {
            let delay = state.next_retry();
            sleep(delay).await;
        } else {
            return error_max_attempts();
        }
        // Otherwise, if the termination condition was met, return the output
        Terminate(result) => return result,
    }
}
```

In the AWS SDK for Rust, waiters can be added without making any backwards breaking changes
to the current API. This doc outlines the approach to add them in this fashion, but does _NOT_
examine code generating response classification from JMESPath expressions, which can be left
to the implementer without concern for the overall API.

Terminology
-----------

Today, there are three layers of `Client` that are easy to confuse, so to make the following easier to follow,
the following terms will be used:

- **Connector**: An implementor of Tower's `Service` trait that converts a request into a response. This is typically
  a thin wrapper around a Hyper client.
- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This isn't intended to be used directly.
- **Fluent Client**: A code generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.
- **AWS Client**: A specialized Fluent Client that uses a `DynConnector`, `DefaultMiddleware`,
  and `Standard` retry policy.

All of these are just called `Client` in code today. This is something that could be clarified in a separate refactor.

Requirements
------------

Waiters must adhere to the [Smithy waiter specification]. To summarize:

1. Waiters are specified by the Smithy `@waitable` trait
2. Retry during polling must be exponential backoff with jitter, with the min/max delay times and
   max attempts configured by the `@waitable` trait
3. The SDK's built-in retry needs to be replaced by the waiter's retry since the Smithy model
   can specify retry conditions that are contrary to the defaults. For example, an error that
   would otherwise be retried by default might be the termination condition for the waiter.
4. Classification of the response must be code generated based on the JMESPath expression in the model.

Waiter API
----------

To invoke a waiter, customers will only need to invoke a single function on the AWS Client. For example,
if waiting for a S3 bucket to exist, it would look like the following:

```rust,ignore
// Request bucket creation
client.create_bucket()
    .bucket_name("my-bucket")
    .send()
    .await()?;

// Wait for it to be created
client.wait_until_bucket_exists()
    .bucket_name("my-bucket")
    .send()
    .await?;
```

The call to `wait_until_bucket_exists()` will return a waiter-specific fluent builder with a `send()` function
that will start the polling and return a future.

To avoid name conflicts with other API methods, the waiter functions can be added to the client via trait:

```rust,ignore
pub trait WaitUntilBucketExists {
    fn wait_until_bucket_exists(&self) -> crate::waiter::bucket_exists::Builder;
}
```

This trait would be implemented for the service's fluent client (which will necessitate making the fluent client's
`handle` field `pub(crate)`).

Waiter Implementation
---------------------

A waiter trait implementation will merely return a fluent builder:

```rust,ignore
impl WaitUntilBucketExists for Client {
    fn wait_until_bucket_exists(&self) -> crate::waiter::bucket_exists::Builder {
        crate::waiter::bucket_exists::Builder::new()
    }
}
```

This builder will have a short `send()` function to kick off the actual waiter implementation:

```rust,ignore
impl Builder {
    // ... existing fluent builder codegen can be reused to create all the setters and constructor

    pub async fn send(self) -> Result<HeadBucketOutput, SdkError<HeadBucketError>> {
        // Builds an input from this builder
        let input = self.inner.build().map_err(|err| aws_smithy_http::result::SdkError::ConstructionFailure(err.into()))?;
        // Passes in the client's handle, which contains a Smithy client and client config
        crate::waiter::bucket_exists::wait(self.handle, input).await
    }
}
```

This wait function needs to, in a loop similar to the pseudo-code in the beginning,
convert the given input into an operation, replace the default response classifier on it
with a no-retry classifier, and then determine what to do next based on that classification:

```rust,ignore
pub async fn wait(
    handle: Arc<Handle<DynConnector, DynMiddleware<DynConnector>, retry::Standard>>,
    input: HeadBucketInput,
) -> Result<HeadBucketOutput, SdkError<HeadBucketError>> {
    loop {
        let operation = input
            .make_operation(&handle.conf)
            .await
            .map_err(|err| {
                aws_smithy_http::result::SdkError::ConstructionFailure(err.into())
            })?;
        // Assume `ClassifyRetry` trait is implemented for `NeverRetry` to always return `RetryKind::Unnecessary`
        let operation = operation.with_retry_classifier(NeverRetry::new());

        let result = handle.client.call(operation).await;
        match classify_result(&input, result) {
            AcceptorState::Retry => {
                // The sleep implementation is available here from `handle.conf.sleep_impl`
                unimplemented!("Check if another attempt should be made and calculate delay time if so")
            }
            AcceptorState::Terminate(output) => return output,
        }
    }
}

fn classify_result(
    input: &HeadBucketInput,
    result: Result<HeadBucketOutput, SdkError<HeadBucketError>>,
) -> AcceptorState<HeadBucketOutput, SdkError<HeadBucketError>> {
    unimplemented!(
        "The Smithy model would dictate conditions to check here to produce an `AcceptorState`"
    )
}
```

The retry delay time should be calculated by the same exponential backoff with jitter code that the
[default `RetryHandler` uses in `aws-smithy-client`]. This function will need to be split up and made
available to the waiter implementations so that just the delay can be calculated.

Changes Checklist
-----------------

- [ ] Codegen fluent builders for waiter input and their `send()` functions
- [ ] Codegen waiter invocation traits
- [ ] Commonize exponential backoff with jitter delay calculation
- [ ] Codegen `wait()` functions with delay and max attempts configuration from Smithy model
- [ ] Codegen `classify_result()` functions based on JMESPath expressions in Smithy model

[Smithy waiter specification]: https://awslabs.github.io/smithy/1.0/spec/waiters.html
[default `RetryHandler` uses in `aws-smithy-client`]: https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-client/src/retry.rs#L252-L292
