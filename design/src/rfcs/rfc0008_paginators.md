## Summary

> Status: Implemented

Smithy [models paginated responses](https://awslabs.github.io/smithy/1.0/spec/core/behavior-traits.html#paginated-trait)
. Customers of Smithy generated code & the Rust SDK will have an improved user experience if code is generated to
support this. Fundamentally, paginators are a way to automatically make a series of requests with the SDK, where subsequent
requests automatically forward output from the previous responses. There is nothing a paginator does that a user could not do manually,
they merely simplify the common task of interacting with paginated APIs. **Specifically, a paginator will resend the orginal request
but with `inputToken` updated to the value of the previous `outputToken`.

In this RFC, we propose modeling paginated data as
a  [`Stream`](https://docs.rs/tokio-stream/0.1.5/tokio_stream/#traits) of output shapes.

- When an output is paginated, a `paginate()` method will be added to the high level builder
- An `<OperationName>Paginator` struct will be generated into the `paginator` module.
- If `items` is modeled, `paginate().items()` will be added to produce the paginated
  items. `<OperationName>PaginatorItems` will be generated into the `paginator` module.

The [`Stream`](https://docs.rs/tokio-stream/latest/tokio_stream/index.html) trait enables customers to use a number of
abstractions including simple looping, and `collect()`ing all data in a single call. A paginator will resend the
original input, but with the field marked `inputToken` to the value of `outputToken` in the previous output.

Usage example:

```rust,ignore
let paginator = client
    .list_tables()
    .paginate()
    .items()
    .page_size(10)
    .send()
    .await;
let tables: Result<Vec<_ >, _ > = paginator.collect().await;
```

Paginators are lazy and only retrieve pages when polled by a client.

### Details

Paginators will be generated into the `paginator` module of service crates. Currently, paginators are _not_ feature gated, but this
could be considered in the future. A `paginator` struct captures 2 pieces of data:

```rust,ignore
// dynamodb/src/paginator.rs
struct ListTablesPaginator<C, M, R> {
    // holds the low-level client and configuration
    handle: Arc<Handle<C, M, R>>,

    // input builder to construct the actual input on demand
    input: ListTablesInputBuilder
}
```

In addition to the basic usage example above, when `pageSize` is modeled, customers can specify the page size during
pagination:

```rust,ignore
let mut tables = vec![];
let mut pages = client
    .list_tables()
    .paginate()
    .page_size(20)
    .send();
while let Some(next_page) = pages.try_next().await? {
    // pages of 20 items requested from DynamoDb
    tables.extend(next_page.table_names.unwrap_or_default().into_iter());
}
```

Paginators define a public method `send()`. This method
returns `impl Stream<Item=Result<OperationOutput, OperationError>`. This uses `FnStream` defined in the `aws-smithy-async` crate which
enables demand driven execution of a closure. A rendezvous channel is used which will block on `send` until demand exists.

When modeled by Smithy, `page_size` which automatically sets the appropriate page_size parameter and `items()` which returns an
automatically flattened paginator are also generated. **Note**: `page_size` directly sets the modeled parameter on the internal builder.
This means that a value set for page size will override any previously set value for that field.
```rust,ignore
// Generated paginator for ListTables
impl<C, M, R> ListTablesPaginator<C, M, R>
{
  /// Set the page size
  pub fn page_size(mut self, limit: i32) -> Self {
    self.builder.limit = Some(limit);
    self
  }

  /// Create a flattened paginator
  ///
  /// This paginator automatically flattens results using `table_names`. Queries to the underlying service
  /// are dispatched lazily.
  pub fn items(self) -> crate::paginator::ListTablesPaginatorItems<C, M, R> {
    crate::paginator::ListTablesPaginatorItems(self)
  }

  /// Create the pagination stream
  ///
  /// _Note:_ No requests will be dispatched until the stream is used (eg. with [`.next().await`](tokio_stream::StreamExt::next)).
  pub async fn send(
    self,
  ) -> impl tokio_stream::Stream<
    Item = std::result::Result<
      crate::output::ListTablesOutput,
      aws_smithy_http::result::SdkError<crate::error::ListTablesError>,
    >,
  > + Unpin
  {
    // Move individual fields out of self for the borrow checker
    let builder = self.builder;
    let handle = self.handle;
    fn_stream::FnStream::new(move |tx| {
      Box::pin(async move {
        // Build the input for the first time. If required fields are missing, this is where we'll produce an early error.
        let mut input = match builder.build().map_err(|err| {
          SdkError::ConstructionFailure(err.into())
        }) {
          Ok(input) => input,
          Err(e) => {
            let _ = tx.send(Err(e)).await;
            return;
          }
        };
        loop {
          let op = match input.make_operation(&handle.conf).await.map_err(|err| {
            SdkError::ConstructionFailure(err.into())
          }) {
            Ok(op) => op,
            Err(e) => {
              let _ = tx.send(Err(e)).await;
              return;
            }
          };
          let resp = handle.client.call(op).await;
          // If the input member is None or it was an error
          let done = match resp {
            Ok(ref resp) => {
              input.exclusive_start_table_name = crate::lens::reflens_structure_crate_output_list_tables_output_last_evaluated_table_name(resp).cloned();
              input.exclusive_start_table_name.is_none()
            }
            Err(_) => true,
          };
          if let Err(_) = tx.send(resp).await {
            // receiving end was dropped
            return;
          }
          if done {
            return;
          }
        }
      })
    })
  }
}
```

**On Box::pin**: The stream returned by `AsyncStream` does not implement `Unpin`. Unfortunately, this makes iteration
require an invocation of `pin_mut!` and generates several hundred lines of compiler errors. Box::pin seems a worthwhile
trade off to improve the user experience.

**On the `+ Unpin` bound**: Because auto-traits leak across `impl Trait` boundaries, `+ Unpin` prevents accidental
regressions in the generated code which would break users.

**On the crate::reflens::...**: We use `LensGenerator.kt` to generate potentially complex accessors to deeply nested fields.

### Updates to ergonomic clients

The `builders` generated by ergonomic clients will gain the following method, if they represent an operation that implements the `Paginated` trait:

```rust,ignore
/// Create a paginator for this request
///
/// Paginators are used by calling [`send().await`](crate::paginator::ListTablesPaginator::send) which returns a [`Stream`](tokio_stream::Stream).
pub fn paginate(self) -> crate::paginator::ListTablesPaginator<C, M, R> {
  crate::paginator::ListTablesPaginator::new(self.handle, self.inner)
}
```

## Discussion Areas
### On `send().await`
Calling `send().await` is not necessary from an API perspective—we could have the paginators impl-stream directly. However,
it enables using `impl Trait` syntax and also makes the API consistent with other SDK APIs.

### On `tokio_stream::Stream`
Currently, the core trait we use is `tokio_stream::Stream`. This is a re-export from futures-util. There are a few other choices:
1. Re-export `Stream` from tokio_stream.
2. Use `futures_util` directly

### On Generics
Currently, the paginators forward the generics from the client (`C, M, R`) along with their fairly annoying bounds.
However, if we wanted to we _could_ simplify this and erase all the generics when the paginator was created. Since everything
is code generated, there isn't actually much duplicated code in the generator, just in the generated code.

## Changes Checklist
- [x] Create and test `FnStream` abstraction
- [x] Generate page-level paginators
- [x] Generate `.items()` paginators
- [x] Generate doc hints pointing people to paginators
- [x] Integration test using mocked HTTP traffic against a generated paginator for a real service
- [ ] Integration test using real traffic
