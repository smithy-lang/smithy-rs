<!-- Do not manually edit this file. Use the `changelogger` tool. -->
March 23rd, 2023
================
**Breaking Changes:**
- ⚠🎉 (all, [smithy-rs#2467](https://github.com/awslabs/smithy-rs/issues/2467)) Update MSRV to 1.66.1
- ⚠ (client, [smithy-rs#76](https://github.com/awslabs/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/awslabs/smithy-rs/issues/2129)) Generic clients no longer expose a `request_id()` function on errors. To get request ID functionality, use the SDK code generator.
- ⚠ (client, [smithy-rs#76](https://github.com/awslabs/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/awslabs/smithy-rs/issues/2129)) The `message()` and `code()` methods on errors have been moved into `ProvideErrorMetadata` trait. This trait will need to be imported to continue calling these.
- ⚠ (client, [smithy-rs#76](https://github.com/awslabs/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/awslabs/smithy-rs/issues/2129), [smithy-rs#2075](https://github.com/awslabs/smithy-rs/issues/2075)) The `*Error` and `*ErrorKind` types have been combined to make error matching simpler.
    <details>
    <summary>Example with S3</summary>
    **Before:**
    ```rust
    let result = client
        .get_object()
        .bucket(BUCKET_NAME)
        .key("some-key")
        .send()
        .await;
    match result {
        Ok(_output) => { /* Do something with the output */ }
        Err(err) => match err.into_service_error() {
            GetObjectError { kind, .. } => match kind {
                GetObjectErrorKind::InvalidObjectState(value) => println!("invalid object state: {:?}", value),
                GetObjectErrorKind::NoSuchKey(_) => println!("object didn't exist"),
            }
            err @ GetObjectError { .. } if err.code() == Some("SomeUnmodeledError") => {}
            err @ _ => return Err(err.into()),
        },
    }
    ```
    **After:**
    ```rust
    // Needed to access the `.code()` function on the error type:
    use aws_sdk_s3::types::ProvideErrorMetadata;
    let result = client
        .get_object()
        .bucket(BUCKET_NAME)
        .key("some-key")
        .send()
        .await;
    match result {
        Ok(_output) => { /* Do something with the output */ }
        Err(err) => match err.into_service_error() {
            GetObjectError::InvalidObjectState(value) => {
                println!("invalid object state: {:?}", value);
            }
            GetObjectError::NoSuchKey(_) => {
                println!("object didn't exist");
            }
            err if err.code() == Some("SomeUnmodeledError") => {}
            err @ _ => return Err(err.into()),
        },
    }
    ```
    </details>
- ⚠ (client, [smithy-rs#76](https://github.com/awslabs/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/awslabs/smithy-rs/issues/2129)) `aws_smithy_types::Error` has been renamed to `aws_smithy_types::error::ErrorMetadata`.
- ⚠ (server, [smithy-rs#2436](https://github.com/awslabs/smithy-rs/issues/2436)) Remove unnecessary type parameter `B` from `Upgrade` service.
- 🐛⚠ (server, [smithy-rs#2382](https://github.com/awslabs/smithy-rs/issues/2382)) Smithy members named `send` were previously renamed to `send_value` at codegen time. These will now be called `send` in the generated code.
- ⚠ (client, [smithy-rs#2448](https://github.com/awslabs/smithy-rs/issues/2448)) The modules in generated client crates have been reorganized. See the [Client Crate Reorganization Upgrade Guidance](https://github.com/awslabs/smithy-rs/discussions/2449) to see how to fix your code after this change.
- ⚠ (server, [smithy-rs#2438](https://github.com/awslabs/smithy-rs/issues/2438)) Servers can send the `ServerRequestId` in the response headers.
    Servers need to create their service using the new layer builder `ServerRequestIdProviderLayer::new_with_response_header`:
    ```
    let app = app
        .layer(&ServerRequestIdProviderLayer::new_with_response_header(HeaderName::from_static("x-request-id")));
    ```

**New this release:**
- 🐛🎉 (client, [aws-sdk-rust#740](https://github.com/awslabs/aws-sdk-rust/issues/740)) Fluent builder methods on the client are now marked as deprecated when the related operation is deprecated.
- 🎉 (all, [smithy-rs#2398](https://github.com/awslabs/smithy-rs/issues/2398)) Add support for the `awsQueryCompatible` trait. This allows services to continue supporting a custom error code (via the `awsQueryError` trait) when the services migrate their protocol from `awsQuery` to `awsJson1_0` annotated with `awsQueryCompatible`.
    <details>
    <summary>Click to expand for more details...</summary>

    After the migration, services will include an additional header `x-amzn-query-error` in their responses whose value is in the form of `<error code>;<error type>`. An example response looks something like
    ```
    HTTP/1.1 400
    x-amzn-query-error: AWS.SimpleQueueService.NonExistentQueue;Sender
    Date: Wed, 08 Sep 2021 23:46:52 GMT
    Content-Type: application/x-amz-json-1.0
    Content-Length: 163

    {
        "__type": "com.amazonaws.sqs#QueueDoesNotExist",
        "message": "some user-visible message"
    }
    ```
    `<error code>` is `AWS.SimpleQueueService.NonExistentQueue` and `<error type>` is `Sender`.

    If an operation results in an error that causes a service to send back the response above, you can access `<error code>` and `<error type>` as follows:
    ```rust
    match client.some_operation().send().await {
        Ok(_) => { /* success */ }
        Err(sdk_err) => {
            let err = sdk_err.into_service_error();
            assert_eq!(
                error.meta().code(),
                Some("AWS.SimpleQueueService.NonExistentQueue"),
            );
            assert_eq!(error.meta().extra("type"), Some("Sender"));
        }
    }
    </details>
    ```
- 🎉 (client, [smithy-rs#2428](https://github.com/awslabs/smithy-rs/issues/2428), [smithy-rs#2208](https://github.com/awslabs/smithy-rs/issues/2208)) `SdkError` variants can now be constructed for easier unit testing.
- 🐛 (server, [smithy-rs#2441](https://github.com/awslabs/smithy-rs/issues/2441)) Fix `FilterByOperationName` plugin. This previous caused services with this applied to fail to compile due to mismatched bounds.
- (client, [smithy-rs#2437](https://github.com/awslabs/smithy-rs/issues/2437), [aws-sdk-rust#600](https://github.com/awslabs/aws-sdk-rust/issues/600)) Add more client re-exports. Specifically, it re-exports `aws_smithy_http::body::SdkBody`, `aws_smithy_http::byte_stream::error::Error`, and `aws_smithy_http::operation::{Request, Response}`.
- 🐛 (all, [smithy-rs#2226](https://github.com/awslabs/smithy-rs/issues/2226)) Fix bug in timestamp format resolution. Prior to this fix, the timestamp format may have been incorrect if set on the target instead of on the member.
- (all, [smithy-rs#2226](https://github.com/awslabs/smithy-rs/issues/2226)) Add support for offsets when parsing datetimes. RFC3339 date times now support offsets like `-0200`
- (client, [aws-sdk-rust#160](https://github.com/awslabs/aws-sdk-rust/issues/160), [smithy-rs#2445](https://github.com/awslabs/smithy-rs/issues/2445)) Reconnect on transient errors.

    Note: **this behavior is disabled by default for generic clients**. It can be enabled with
    `aws_smithy_client::Builder::reconnect_on_transient_errors`

    If a transient error (timeout, 500, 503, 503) is encountered, the connection will be evicted from the pool and will not
    be reused.
- (all, [smithy-rs#2474](https://github.com/awslabs/smithy-rs/issues/2474)) Increase Tokio version to 1.23.1 for all crates. This is to address [RUSTSEC-2023-0001](https://rustsec.org/advisories/RUSTSEC-2023-0001)


January 25th, 2023
==================
**New this release:**
- 🐛 (server, [smithy-rs#920](https://github.com/awslabs/smithy-rs/issues/920)) Fix bug in `OperationExtensionFuture`s `Future::poll` implementation


January 24th, 2023
==================
**Breaking Changes:**
- ⚠ (server, [smithy-rs#2161](https://github.com/awslabs/smithy-rs/issues/2161)) Remove deprecated service builder, this includes:

    - Remove `aws_smithy_http_server::routing::Router` and `aws_smithy_http_server::request::RequestParts`.
    - Move the `aws_smithy_http_server::routers::Router` trait and `aws_smithy_http_server::routing::RoutingService` into `aws_smithy_http_server::routing`.
    - Remove the following from the generated SDK:
        - `operation_registry.rs`
        - `operation_handler.rs`
        - `server_operation_handler_trait.rs`

    If migration to the new service builder API has not already been completed a brief summary of required changes can be seen in [previous release notes](https://github.com/awslabs/smithy-rs/releases/tag/release-2022-12-12) and in API documentation of the root crate.

**New this release:**
- 🐛 (server, [smithy-rs#2213](https://github.com/awslabs/smithy-rs/issues/2213)) `@sparse` list shapes and map shapes with constraint traits and with constrained members are now supported
- 🐛 (server, [smithy-rs#2200](https://github.com/awslabs/smithy-rs/pull/2200)) Event streams no longer generate empty error enums when their operations don’t have modeled errors
- (all, [smithy-rs#2223](https://github.com/awslabs/smithy-rs/issues/2223)) `aws_smithy_types::date_time::DateTime`, `aws_smithy_types::Blob` now implement the `Eq` and `Hash` traits
- (server, [smithy-rs#2223](https://github.com/awslabs/smithy-rs/issues/2223)) Code-generated types for server SDKs now implement the `Eq` and `Hash` traits when possible


January 12th, 2023
==================
**New this release:**
- 🐛 (server, [smithy-rs#2201](https://github.com/awslabs/smithy-rs/issues/2201)) Fix severe bug where a router fails to deserialize percent-encoded query strings, reporting no operation match when there could be one. If your Smithy model uses an operation with a request URI spec containing [query string literals](https://smithy.io/2.0/spec/http-bindings.html#query-string-literals), you are affected. This fix was released in `aws-smithy-http-server` v0.53.1.


January 11th, 2023
==================
**Breaking Changes:**
- ⚠ (client, [smithy-rs#2099](https://github.com/awslabs/smithy-rs/issues/2099)) The Rust client codegen plugin is now called `rust-client-codegen` instead of `rust-codegen`. Be sure to update your `smithy-build.json` files to refer to the correct plugin name.
- ⚠ (client, [smithy-rs#2099](https://github.com/awslabs/smithy-rs/issues/2099)) Client codegen plugins need to define a service named `software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator` (this is the new file name for the plugin definition in `resources/META-INF/services`).
- ⚠ (server, [smithy-rs#2099](https://github.com/awslabs/smithy-rs/issues/2099)) Server codegen plugins need to define a service named `software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator` (this is the new file name for the plugin definition in `resources/META-INF/services`).

**New this release:**
- 🐛 (server, [smithy-rs#2103](https://github.com/awslabs/smithy-rs/issues/2103)) In 0.52, `@length`-constrained collection shapes whose members are not constrained made the server code generator crash. This has been fixed.
- (server, [smithy-rs#1879](https://github.com/awslabs/smithy-rs/issues/1879)) Servers support the `@default` trait: models can specify default values. Default values will be automatically supplied when not manually set.
- (server, [smithy-rs#2131](https://github.com/awslabs/smithy-rs/issues/2131)) The constraint `@length` on non-streaming blob shapes is supported.
- 🐛 (client, [smithy-rs#2150](https://github.com/awslabs/smithy-rs/issues/2150)) Fix bug where string default values were not supported for endpoint parameters
- 🐛 (all, [smithy-rs#2170](https://github.com/awslabs/smithy-rs/issues/2170), [aws-sdk-rust#706](https://github.com/awslabs/aws-sdk-rust/issues/706)) Remove the webpki-roots feature from `hyper-rustls`
- 🐛 (server, [smithy-rs#2054](https://github.com/awslabs/smithy-rs/issues/2054)) Servers can generate a unique request ID and use it in their handlers.


December 12th, 2022
===================
**Breaking Changes:**
- ⚠🎉 (all, [smithy-rs#1938](https://github.com/awslabs/smithy-rs/issues/1938), @jjantdev) Upgrade Rust MSRV to 1.62.1
- ⚠🎉 (server, [smithy-rs#1199](https://github.com/awslabs/smithy-rs/issues/1199), [smithy-rs#1342](https://github.com/awslabs/smithy-rs/issues/1342), [smithy-rs#1401](https://github.com/awslabs/smithy-rs/issues/1401), [smithy-rs#1998](https://github.com/awslabs/smithy-rs/issues/1998), [smithy-rs#2005](https://github.com/awslabs/smithy-rs/issues/2005), [smithy-rs#2028](https://github.com/awslabs/smithy-rs/issues/2028), [smithy-rs#2034](https://github.com/awslabs/smithy-rs/issues/2034), [smithy-rs#2036](https://github.com/awslabs/smithy-rs/issues/2036)) [Constraint traits](https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html) in server SDKs are beginning to be supported. The following are now supported:

    * The `length` trait on `string` shapes.
    * The `length` trait on `map` shapes.
    * The `length` trait on `list` shapes.
    * The `range` trait on `byte` shapes.
    * The `range` trait on `short` shapes.
    * The `range` trait on `integer` shapes.
    * The `range` trait on `long` shapes.
    * The `pattern` trait on `string` shapes.

    Upon receiving a request that violates the modeled constraints, the server SDK will reject it with a message indicating why.

    Unsupported (constraint trait, target shape) combinations will now fail at code generation time, whereas previously they were just ignored. This is a breaking change to raise awareness in service owners of their server SDKs behaving differently than what was modeled. To continue generating a server SDK with unsupported constraint traits, set `codegen.ignoreUnsupportedConstraints` to `true` in your `smithy-build.json`.

    ```json
    {
        ...
        "rust-server-codegen": {
            ...
            "codegen": {
                "ignoreUnsupportedConstraints": true
            }
        }
    }
    ```
- ⚠🎉 (server, [smithy-rs#1342](https://github.com/awslabs/smithy-rs/issues/1342), [smithy-rs#1119](https://github.com/awslabs/smithy-rs/issues/1119)) Server SDKs now generate "constrained types" for constrained shapes. Constrained types are [newtypes](https://rust-unofficial.github.io/patterns/patterns/behavioural/newtype.html) that encapsulate the modeled constraints. They constitute a [widespread pattern to guarantee domain invariants](https://www.lpalmieri.com/posts/2020-12-11-zero-to-production-6-domain-modelling/) and promote correctness in your business logic. So, for example, the model:

    ```smithy
    @length(min: 1, max: 69)
    string NiceString
    ```

    will now render a `struct NiceString(String)`. Instantiating a `NiceString` is a fallible operation:

    ```rust
    let data: String = ... ;
    let nice_string = NiceString::try_from(data).expect("data is not nice");
    ```

    A failed attempt to instantiate a constrained type will yield a `ConstraintViolation` error type you may want to handle. This type's API is subject to change.

    Constrained types _guarantee_, by virtue of the type system, that your service's operation outputs adhere to the modeled constraints. To learn more about the motivation for constrained types and how they work, see [the RFC](https://github.com/awslabs/smithy-rs/pull/1199).

    If you'd like to opt-out of generating constrained types, you can set `codegen.publicConstrainedTypes` to `false`. Note that if you do, the generated server SDK will still honor your operation input's modeled constraints upon receiving a request, but will not help you in writing business logic code that adheres to the constraints, and _will not prevent you from returning responses containing operation outputs that violate said constraints_.

    ```json
    {
        ...
        "rust-server-codegen": {
            ...
            "codegen": {
                "publicConstrainedTypes": false
            }
        }
    }
    ```
- 🐛⚠🎉 (server, [smithy-rs#1714](https://github.com/awslabs/smithy-rs/issues/1714), [smithy-rs#1342](https://github.com/awslabs/smithy-rs/issues/1342)) Structure builders in server SDKs have undergone significant changes.

    The API surface has been reduced. It is now simpler and closely follows what you would get when using the [`derive_builder`](https://docs.rs/derive_builder/latest/derive_builder/) crate:

    1. Builders no longer have `set_*` methods taking in `Option<T>`. You must use the unprefixed method, named exactly after the structure's field name, and taking in a value _whose type matches exactly that of the structure's field_.
    2. Builders no longer have convenience methods to pass in an element for a field whose type is a vector or a map. You must pass in the entire contents of the collection up front.
    3. Builders no longer implement [`PartialEq`](https://doc.rust-lang.org/std/cmp/trait.PartialEq.html).

    Bug fixes:

    4. Builders now always fail to build if a value for a `required` member is not provided. Previously, builders were falling back to a default value (e.g. `""` for `String`s) for some shapes. This was a bug.

    Additions:

    5. A structure `Structure` with builder `Builder` now implements `TryFrom<Builder> for Structure` or `From<Builder> for Structure`, depending on whether the structure [is constrained](https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html) or not, respectively.

    To illustrate how to migrate to the new API, consider the example model below.

    ```smithy
    structure Pokemon {
        @required
        name: String,
        @required
        description: String,
        @required
        evolvesTo: PokemonList
    }

    list PokemonList {
        member: Pokemon
    }
    ```

    In the Rust code below, note the references calling out the changes described in the numbered list above.

    Before:

    ```rust
    let eevee_builder = Pokemon::builder()
        // (1) `set_description` takes in `Some<String>`.
        .set_description(Some("Su código genético es muy inestable. Puede evolucionar en diversas razas de Pokémon.".to_owned()))
        // (2) Convenience method to add one element to the `evolvesTo` list.
        .evolves_to(vaporeon)
        .evolves_to(jolteon)
        .evolves_to(flareon);

    // (3) Builder types can be compared.
    assert_ne!(eevee_builder, Pokemon::builder());

    // (4) Builds fine even though we didn't provide a value for `name`, which is `required`!
    let _eevee = eevee_builder.build();
    ```

    After:

    ```rust
    let eevee_builder = Pokemon::builder()
        // (1) `set_description` no longer exists. Use `description`, which directly takes in `String`.
        .description("Su código genético es muy inestable. Puede evolucionar en diversas razas de Pokémon.".to_owned())
        // (2) Convenience methods removed; provide the entire collection up front.
        .evolves_to(vec![vaporeon, jolteon, flareon]);

    // (3) Binary operation `==` cannot be applied to `pokemon::Builder`.
    // assert_ne!(eevee_builder, Pokemon::builder());

    // (4) `required` member `name` was not set.
    // (5) Builder type can be fallibly converted to the structure using `TryFrom` or `TryInto`.
    let _error = Pokemon::try_from(eevee_builder).expect_err("name was not provided");
    ```
- ⚠🎉 (server, [smithy-rs#1620](https://github.com/awslabs/smithy-rs/issues/1620), [smithy-rs#1666](https://github.com/awslabs/smithy-rs/issues/1666), [smithy-rs#1731](https://github.com/awslabs/smithy-rs/issues/1731), [smithy-rs#1736](https://github.com/awslabs/smithy-rs/issues/1736), [smithy-rs#1753](https://github.com/awslabs/smithy-rs/issues/1753), [smithy-rs#1738](https://github.com/awslabs/smithy-rs/issues/1738), [smithy-rs#1782](https://github.com/awslabs/smithy-rs/issues/1782), [smithy-rs#1829](https://github.com/awslabs/smithy-rs/issues/1829), [smithy-rs#1837](https://github.com/awslabs/smithy-rs/issues/1837), [smithy-rs#1891](https://github.com/awslabs/smithy-rs/issues/1891), [smithy-rs#1840](https://github.com/awslabs/smithy-rs/issues/1840), [smithy-rs#1844](https://github.com/awslabs/smithy-rs/issues/1844), [smithy-rs#1858](https://github.com/awslabs/smithy-rs/issues/1858), [smithy-rs#1930](https://github.com/awslabs/smithy-rs/issues/1930), [smithy-rs#1999](https://github.com/awslabs/smithy-rs/issues/1999), [smithy-rs#2003](https://github.com/awslabs/smithy-rs/issues/2003), [smithy-rs#2008](https://github.com/awslabs/smithy-rs/issues/2008), [smithy-rs#2010](https://github.com/awslabs/smithy-rs/issues/2010), [smithy-rs#2019](https://github.com/awslabs/smithy-rs/issues/2019), [smithy-rs#2020](https://github.com/awslabs/smithy-rs/issues/2020), [smithy-rs#2021](https://github.com/awslabs/smithy-rs/issues/2021), [smithy-rs#2038](https://github.com/awslabs/smithy-rs/issues/2038), [smithy-rs#2039](https://github.com/awslabs/smithy-rs/issues/2039), [smithy-rs#2041](https://github.com/awslabs/smithy-rs/issues/2041)) ### Plugins/New Service Builder API

    The `Router` struct has been replaced by a new `Service` located at the root of the generated crate. Its name coincides with the same name as the Smithy service you are generating.

    ```rust
    use pokemon_service_server_sdk::PokemonService;
    ```

    The new service builder infrastructure comes with a `Plugin` system which supports middleware on `smithy-rs`. See the [mididleware documentation](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/middleware.md) and the [API documentation](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/plugin/index.html) for more details.

    Usage of the new service builder API:

    ```rust
    // Apply a sequence of plugins using `PluginPipeline`.
    let plugins = PluginPipeline::new()
        // Apply the `PrintPlugin`.
        // This is a dummy plugin found in `rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/plugin.rs`
        .print()
        // Apply the `InstrumentPlugin` plugin, which applies `tracing` instrumentation.
        .instrument();

    // Construct the service builder using the `plugins` defined above.
    let app = PokemonService::builder_with_plugins(plugins)
        // Assign all the handlers.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        // Construct the `PokemonService`.
        .build()
        // If handlers are missing a descriptive error will be provided.
        .expect("failed to build an instance of `PokemonService`");
    ```

    See the `rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/bin` folder for various working examples.

    ### Public `FromParts` trait

    Previously, we only supported one [`Extension`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/struct.Extension.html) as an additional argument provided to the handler. This number has been increased to 8 and the argument type has been broadened to any struct which implements the [`FromParts`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/trait.FromParts.html) trait. The trait is publicly exported and therefore provides customers with the ability to extend the domain of the handlers.

    As noted, a ubiqutious example of a struct that implements `FromParts` is the `Extension` struct, which extracts state from the `Extensions` typemap of a [`http::Request`](https://docs.rs/http/latest/http/request/struct.Request.html). A new example is the `ConnectInfo` struct which allows handlers to access the connection data. See the `rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/bin/pokemon-service-connect-info.rs` example.

    ```rust
    fn get_pokemon_species(
        input: GetPokemonSpeciesInput,
        state: Extension<State>,
        address: ConnectInfo<SocketAddr>
    ) -> Result<GetPokemonSpeciesOutput, GetPokemonSpeciesError> {
        todo!()
    }
    ```

    In addition to the [`ConnectInfo`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/connect_info/struct.ConnectInfo.html) extractor, we also have added [lambda extractors](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/lambda/index.html) which are feature gated with `aws-lambda`.

    [`FromParts` documentation](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/from_parts.md) has been added.

    ### New Documentation

    New sections to have been added to the [server side of the book](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/overview.md).

    These include:

    - [Middleware](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/middleware.md)
    - [Accessing Un-modelled Data](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/from_parts.md)
    - [Anatomy of a Service](https://github.com/awslabs/smithy-rs/blob/main/design/src/server/anatomy.md)

    This release also introduces extensive documentation at the root of the generated crate. For best results compile documentation with `cargo +nightly doc --open`.

    ### Deprecations

    The existing service builder infrastructure, `OperationRegistryBuilder`/`OperationRegistry`/`Router`, is now deprecated. Customers should migrate to the newer scheme described above. The deprecated types will be removed in a future release.
- ⚠ (client, [smithy-rs#1875](https://github.com/awslabs/smithy-rs/issues/1875)) Replace bool with enum for a function parameter of `label::fmt_string`.
- ⚠ (all, [smithy-rs#1980](https://github.com/awslabs/smithy-rs/issues/1980)) aws_smithy_types_convert::date_time::DateTimeExt::to_chrono_utc returns a Result<>
- ⚠ (client, [smithy-rs#1926](https://github.com/awslabs/smithy-rs/issues/1926), [smithy-rs#1819](https://github.com/awslabs/smithy-rs/issues/1819)) Several breaking changes have been made to errors. See [the upgrade guide](https://github.com/awslabs/smithy-rs/issues/1950) for more information.
- 🐛⚠ (server, [smithy-rs#1714](https://github.com/awslabs/smithy-rs/issues/1714), [smithy-rs#1342](https://github.com/awslabs/smithy-rs/issues/1342), [smithy-rs#1860](https://github.com/awslabs/smithy-rs/issues/1860)) Server SDKs now correctly reject operation inputs that don't set values for `required` structure members. Previously, in some scenarios, server SDKs would accept the request and set a default value for the member (e.g. `""` for a `String`), even when the member shape did not have [Smithy IDL v2's `default` trait](https://awslabs.github.io/smithy/2.0/spec/type-refinement-traits.html#smithy-api-default-trait) attached. The `default` trait is [still unsupported](https://github.com/awslabs/smithy-rs/issues/1860).
- ⚠ (client, [smithy-rs#1945](https://github.com/awslabs/smithy-rs/issues/1945)) Generate enums that guide the users to write match expressions in a forward-compatible way.
    Before this change, users could write a match expression against an enum in a non-forward-compatible way:
    ```rust
    match some_enum {
        SomeEnum::Variant1 => { /* ... */ },
        SomeEnum::Variant2 => { /* ... */ },
        Unknown(value) if value == "NewVariant" => { /* ... */ },
        _ => { /* ... */ },
    }
    ```
    This code can handle a case for "NewVariant" with a version of SDK where the enum does not yet include `SomeEnum::NewVariant`, but breaks with another version of SDK where the enum defines `SomeEnum::NewVariant` because the execution will hit a different match arm, i.e. the last one.
    After this change, users are guided to write the above match expression as follows:
    ```rust
    match some_enum {
        SomeEnum::Variant1 => { /* ... */ },
        SomeEnum::Variant2 => { /* ... */ },
        other @ _ if other.as_str() == "NewVariant" => { /* ... */ },
        _ => { /* ... */ },
    }
    ```
    This is forward-compatible because the execution will hit the second last match arm regardless of whether the enum defines `SomeEnum::NewVariant` or not.
- ⚠ (client, [smithy-rs#1984](https://github.com/awslabs/smithy-rs/issues/1984), [smithy-rs#1496](https://github.com/awslabs/smithy-rs/issues/1496)) Functions on `aws_smithy_http::endpoint::Endpoint` now return a `Result` instead of panicking.
- ⚠ (client, [smithy-rs#1984](https://github.com/awslabs/smithy-rs/issues/1984), [smithy-rs#1496](https://github.com/awslabs/smithy-rs/issues/1496)) `Endpoint::mutable` now takes `impl AsRef<str>` instead of `Uri`. For the old functionality, use `Endpoint::mutable_uri`.
- ⚠ (client, [smithy-rs#1984](https://github.com/awslabs/smithy-rs/issues/1984), [smithy-rs#1496](https://github.com/awslabs/smithy-rs/issues/1496)) `Endpoint::immutable` now takes `impl AsRef<str>` instead of `Uri`. For the old functionality, use `Endpoint::immutable_uri`.
- ⚠ (server, [smithy-rs#1982](https://github.com/awslabs/smithy-rs/issues/1982)) [RestJson1](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html#operation-error-serialization) server SDKs now serialize the [full shape ID](https://smithy.io/2.0/spec/model.html#shape-id) (including namespace) in operation error responses.

    Example server error response before:

    ```
    HTTP/1.1 400 Bad Request
    content-type: application/json
    x-amzn-errortype: InvalidRequestException
    ...
    ```

    Example server error response now:

    ```
    HTTP/1.1 400 Bad Request
    content-type: application/json
    x-amzn-errortype: com.example.service#InvalidRequestException
    ...
    ```
- ⚠ (server, [smithy-rs#2035](https://github.com/awslabs/smithy-rs/issues/2035)) All types that are exclusively relevant within the context of an AWS Lambda function are now gated behind the
    `aws-lambda` feature flag.

    This will reduce the number of dependencies (and improve build times) for users that are running their Smithy services
    in non-serverless environments (e.g. via `hyper`).
- ⚠ (all, [smithy-rs#1983](https://github.com/awslabs/smithy-rs/issues/1983), [smithy-rs#2029](https://github.com/awslabs/smithy-rs/issues/2029)) Implementation of the Debug trait for container shapes now redacts what is printed per the sensitive trait.
- ⚠ (client, [smithy-rs#2065](https://github.com/awslabs/smithy-rs/issues/2065)) `SdkBody` callbacks have been removed. If you were using these, please [file an issue](https://github.com/awslabs/smithy-rs/issues/new) so that we can better understand your use-case and provide the support you need.
- ⚠ (client, [smithy-rs#2063](https://github.com/awslabs/smithy-rs/issues/2063)) Added SmithyEndpointStage which can be used to set an endpoint for smithy-native clients
- ⚠ (all, [smithy-rs#1989](https://github.com/awslabs/smithy-rs/issues/1989)) The Unit type for a Union member is no longer rendered. The serializers and parsers generated now function accordingly in the absence of the inner data associated with the Unit type.

**New this release:**
- 🎉 (all, [smithy-rs#1929](https://github.com/awslabs/smithy-rs/issues/1929)) Upgrade Smithy to v1.26
- 🎉 (client, [smithy-rs#2044](https://github.com/awslabs/smithy-rs/issues/2044), [smithy-rs#371](https://github.com/awslabs/smithy-rs/issues/371)) Fixed and improved the request `tracing` span hierarchy to improve log messages, profiling, and debuggability.
- 🐛 (all, [smithy-rs#1847](https://github.com/awslabs/smithy-rs/issues/1847)) Support Sigv4 signature generation on PowerPC 32 and 64 bit. This architecture cannot compile `ring`, so the implementation has been updated to rely on `hamc` + `sha2` to achive the same result with broader platform compatibility and higher performance. We also updated the CI which is now running as many tests as possible against i686 and PowerPC 32 and 64 bit.
- 🐛 (server, [smithy-rs#1910](https://github.com/awslabs/smithy-rs/issues/1910)) `aws_smithy_http_server::routing::Router` is exported from the crate root again. This reverts unintentional breakage that was introduced in `aws-smithy-http-server` v0.51.0 only.
- 🐛 (client, [smithy-rs#1903](https://github.com/awslabs/smithy-rs/issues/1903), [smithy-rs#1902](https://github.com/awslabs/smithy-rs/issues/1902)) Fix bug that can cause panics in paginators
- (client, [smithy-rs#1919](https://github.com/awslabs/smithy-rs/issues/1919)) Operation metadata is now added to the property bag before sending requests allowing middlewares to behave
    differently depending on the operation being sent.
- (all, [smithy-rs#1907](https://github.com/awslabs/smithy-rs/issues/1907)) Fix cargo audit issue on chrono.
- 🐛 (client, [smithy-rs#1957](https://github.com/awslabs/smithy-rs/issues/1957)) It was previously possible to send requests without setting query parameters modeled as required. Doing this may cause a
    service to interpret a request incorrectly instead of just sending back a 400 error. Now, when an operation has query
    parameters that are marked as required, the omission of those query parameters will cause a BuildError, preventing the
    invalid operation from being sent.
- (all, [smithy-rs#1972](https://github.com/awslabs/smithy-rs/issues/1972)) Upgrade to Smithy 1.26.2
- (all, [smithy-rs#2011](https://github.com/awslabs/smithy-rs/issues/2011), @lsr0) Make generated enum `values()` functions callable in const contexts.
- (client, [smithy-rs#2064](https://github.com/awslabs/smithy-rs/issues/2064), [aws-sdk-rust#632](https://github.com/awslabs/aws-sdk-rust/issues/632)) Clients now default max idle connections to 70 (previously unlimited) to reduce the likelihood of hitting max file handles in AWS Lambda.
- (client, [smithy-rs#2057](https://github.com/awslabs/smithy-rs/issues/2057), [smithy-rs#371](https://github.com/awslabs/smithy-rs/issues/371)) Add more `tracing` events to signing and event streams

**Contributors**
Thank you for your contributions! ❤
- @jjantdev ([smithy-rs#1938](https://github.com/awslabs/smithy-rs/issues/1938))
- @lsr0 ([smithy-rs#2011](https://github.com/awslabs/smithy-rs/issues/2011))

October 24th, 2022
==================
**Breaking Changes:**
- ⚠ (all, [smithy-rs#1825](https://github.com/awslabs/smithy-rs/issues/1825)) Bump MSRV to be 1.62.0.
- ⚠ (server, [smithy-rs#1825](https://github.com/awslabs/smithy-rs/issues/1825)) Bump pyo3 and pyo3-asyncio from 0.16.x to 0.17.0 for aws-smithy-http-server-python.
- ⚠ (client, [smithy-rs#1811](https://github.com/awslabs/smithy-rs/issues/1811)) Replace all usages of `AtomicU64` with `AtomicUsize` to support 32bit targets.
- ⚠ (server, [smithy-rs#1803](https://github.com/awslabs/smithy-rs/issues/1803)) Mark `operation` and `operation_handler` modules as private in the generated server crate.
    Both modules did not contain any public types, therefore there should be no actual breakage when updating.
- ⚠ (client, [smithy-rs#1740](https://github.com/awslabs/smithy-rs/issues/1740), [smithy-rs#256](https://github.com/awslabs/smithy-rs/issues/256)) A large list of breaking changes were made to accomodate default timeouts in the AWS SDK.
    See [the smithy-rs upgrade guide](https://github.com/awslabs/smithy-rs/issues/1760) for a full list
    of breaking changes and how to resolve them.
- ⚠ (server, [smithy-rs#1829](https://github.com/awslabs/smithy-rs/issues/1829)) Remove `Protocol` enum, removing an obstruction to extending smithy to third-party protocols.
- ⚠ (server, [smithy-rs#1829](https://github.com/awslabs/smithy-rs/issues/1829)) Convert the `protocol` argument on `PyMiddlewares::new` constructor to a type parameter.
- ⚠ (server, [smithy-rs#1753](https://github.com/awslabs/smithy-rs/issues/1753)) `aws_smithy_http_server::routing::Router` is no longer exported from the crate root. This was unintentional breakage that will be reverted in the next release.

**New this release:**
- (server, [smithy-rs#1811](https://github.com/awslabs/smithy-rs/issues/1811)) Replace all usages of `AtomicU64` with `AtomicUsize` to support 32bit targets.
- 🐛 (all, [smithy-rs#1802](https://github.com/awslabs/smithy-rs/issues/1802)) Sensitive fields in errors now respect @sensitive trait and are properly redacted.
- (server, [smithy-rs#1727](https://github.com/awslabs/smithy-rs/issues/1727), @GeneralSwiss) Pokémon Service example code now runs clippy during build.
- (server, [smithy-rs#1734](https://github.com/awslabs/smithy-rs/issues/1734)) Implement support for pure Python request middleware. Improve idiomatic logging support over tracing.
- 🐛 (client, [aws-sdk-rust#620](https://github.com/awslabs/aws-sdk-rust/issues/620), [smithy-rs#1748](https://github.com/awslabs/smithy-rs/issues/1748)) Paginators now stop on encountering a duplicate token by default rather than panic. This behavior can be customized by toggling the `stop_on_duplicate_token` property on the paginator before calling `send`.
- 🐛 (all, [smithy-rs#1817](https://github.com/awslabs/smithy-rs/issues/1817), @ethyi) Update aws-types zeroize to flexible version to prevent downstream version conflicts.
- (all, [smithy-rs#1852](https://github.com/awslabs/smithy-rs/issues/1852), @ogudavid) Enable local maven repo dependency override.

**Contributors**
Thank you for your contributions! ❤
- @GeneralSwiss ([smithy-rs#1727](https://github.com/awslabs/smithy-rs/issues/1727))
- @ethyi ([smithy-rs#1817](https://github.com/awslabs/smithy-rs/issues/1817))
- @ogudavid ([smithy-rs#1852](https://github.com/awslabs/smithy-rs/issues/1852))

September 20th, 2022
====================
**Breaking Changes:**
- ⚠ (client, [smithy-rs#1603](https://github.com/awslabs/smithy-rs/issues/1603), [aws-sdk-rust#586](https://github.com/awslabs/aws-sdk-rust/issues/586)) `aws_smithy_types::RetryConfig` no longer implements `Default`, and its `new` function has been replaced with `standard`.
- ⚠ (client, [smithy-rs#1603](https://github.com/awslabs/smithy-rs/issues/1603), [aws-sdk-rust#586](https://github.com/awslabs/aws-sdk-rust/issues/586)) Client creation now panics if retries or timeouts are enabled without an async sleep implementation.
    If you're using the Tokio runtime and have the `rt-tokio` feature enabled (which is enabled by default),
    then you shouldn't notice this change at all.
    Otherwise, if using something other than Tokio as the async runtime, the `AsyncSleep` trait must be implemented,
    and that implementation given to the config builder via the `sleep_impl` method. Alternatively, retry can be
    explicitly turned off by setting `max_attempts` to 1, which will result in successful client creation without an
    async sleep implementation.
- ⚠ (client, [smithy-rs#1603](https://github.com/awslabs/smithy-rs/issues/1603), [aws-sdk-rust#586](https://github.com/awslabs/aws-sdk-rust/issues/586)) The `default_async_sleep` method on the `Client` builder has been removed. The default async sleep is
    wired up by default if none is provided.
- ⚠ (client, [smithy-rs#976](https://github.com/awslabs/smithy-rs/issues/976), [smithy-rs#1710](https://github.com/awslabs/smithy-rs/issues/1710)) Removed the need to generate operation output and retry aliases in codegen.
- ⚠ (client, [smithy-rs#1715](https://github.com/awslabs/smithy-rs/issues/1715), [smithy-rs#1717](https://github.com/awslabs/smithy-rs/issues/1717)) `ClassifyResponse` was renamed to `ClassifyRetry` and is no longer implemented for the unit type.
- ⚠ (client, [smithy-rs#1715](https://github.com/awslabs/smithy-rs/issues/1715), [smithy-rs#1717](https://github.com/awslabs/smithy-rs/issues/1717)) The `with_retry_policy` and `retry_policy` functions on `aws_smithy_http::operation::Operation` have been
    renamed to `with_retry_classifier` and `retry_classifier` respectively. Public member `retry_policy` on
    `aws_smithy_http::operation::Parts` has been renamed to `retry_classifier`.

**New this release:**
- 🎉 (client, [smithy-rs#1647](https://github.com/awslabs/smithy-rs/issues/1647), [smithy-rs#1112](https://github.com/awslabs/smithy-rs/issues/1112)) Implemented customizable operations per [RFC-0017](https://awslabs.github.io/smithy-rs/design/rfcs/rfc0017_customizable_client_operations.html).

    Before this change, modifying operations before sending them required using lower-level APIs:

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

    Now, users may easily modify operations before sending with the `customize` method:

    ```rust
    let response = client.some_operation()
        .some_value(5)
        .customize()
        .await?
        .mutate_request(|mut req| {
            req.headers_mut().insert(
                HeaderName::from_static("x-some-header"),
                HeaderValue::from_static("some-value")
            );
        })
        .send()
        .await?;
    ```
- (client, [smithy-rs#1735](https://github.com/awslabs/smithy-rs/issues/1735), @vojtechkral) Lower log level of two info-level log messages.
- (all, [smithy-rs#1710](https://github.com/awslabs/smithy-rs/issues/1710)) Added `writable` property to `RustType` and `RuntimeType` that returns them in `Writable` form
- (all, [smithy-rs#1680](https://github.com/awslabs/smithy-rs/issues/1680), @ogudavid) Smithy IDL v2 mixins are now supported
- 🐛 (client, [smithy-rs#1715](https://github.com/awslabs/smithy-rs/issues/1715), [smithy-rs#1717](https://github.com/awslabs/smithy-rs/issues/1717)) Generated clients now retry transient errors without replacing the retry policy.
- 🐛 (all, [smithy-rs#1725](https://github.com/awslabs/smithy-rs/issues/1725), @sugmanue) Correctly determine nullability of members in IDLv2 models

**Contributors**
Thank you for your contributions! ❤
- @ogudavid ([smithy-rs#1680](https://github.com/awslabs/smithy-rs/issues/1680))
- @sugmanue ([smithy-rs#1725](https://github.com/awslabs/smithy-rs/issues/1725))
- @vojtechkral ([smithy-rs#1735](https://github.com/awslabs/smithy-rs/issues/1735))

August 31st, 2022
=================
**Breaking Changes:**
- ⚠🎉 (client, [smithy-rs#1598](https://github.com/awslabs/smithy-rs/issues/1598)) Previously, the config customizations that added functionality related to retry configs, timeout configs, and the
    async sleep impl were defined in the smithy codegen module but were being loaded in the AWS codegen module. They
    have now been updated to be loaded during smithy codegen. The affected classes are all defined in the
    `software.amazon.smithy.rust.codegen.smithy.customizations` module of smithy codegen.` This change does not affect
    the generated code.

    These classes have been removed:
    - `RetryConfigDecorator`
    - `SleepImplDecorator`
    - `TimeoutConfigDecorator`

    These classes have been renamed:
    - `RetryConfigProviderConfig` is now `RetryConfigProviderCustomization`
    - `PubUseRetryConfig` is now `PubUseRetryConfigGenerator`
    - `SleepImplProviderConfig` is now `SleepImplProviderCustomization`
    - `TimeoutConfigProviderConfig` is now `TimeoutConfigProviderCustomization`
- ⚠🎉 (all, [smithy-rs#1635](https://github.com/awslabs/smithy-rs/issues/1635), [smithy-rs#1416](https://github.com/awslabs/smithy-rs/issues/1416), @weihanglo) Support granular control of specifying runtime crate versions.

    For code generation, the field `runtimeConfig.version` in smithy-build.json has been removed.
    The new field `runtimeConfig.versions` is an object whose keys are runtime crate names (e.g. `aws-smithy-http`),
    and values are user-specified versions.

    If you previously set `version = "DEFAULT"`, the migration path is simple.
    By setting `versions` with an empty object or just not setting it at all,
    the version number of the code generator will be used as the version for all runtime crates.

    If you specified a certain version such as `version = "0.47.0", you can migrate to a special reserved key `DEFAULT`.
    The equivalent JSON config would look like:

    ```json
    {
      "runtimeConfig": {
          "versions": {
              "DEFAULT": "0.47.0"
          }
      }
    }
    ```

    Then all runtime crates are set with version 0.47.0 by default unless overridden by specific crates. For example,

    ```json
    {
      "runtimeConfig": {
          "versions": {
              "DEFAULT": "0.47.0",
              "aws-smithy-http": "0.47.1"
          }
      }
    }
    ```

    implies that we're using `aws-smithy-http` 0.47.1 specifically. For the rest of the crates, it will default to 0.47.0.
- ⚠ (all, [smithy-rs#1623](https://github.com/awslabs/smithy-rs/issues/1623), @ogudavid) Remove @sensitive trait tests which applied trait to member. The ability to mark members with @sensitive was removed in Smithy 1.22.
- ⚠ (server, [smithy-rs#1544](https://github.com/awslabs/smithy-rs/issues/1544)) Servers now allow requests' ACCEPT header values to be:
    - `*/*`
    - `type/*`
    - `type/subtype`
- 🐛⚠ (all, [smithy-rs#1274](https://github.com/awslabs/smithy-rs/issues/1274)) Lossy converters into integer types for `aws_smithy_types::Number` have been
    removed. Lossy converters into floating point types for
    `aws_smithy_types::Number` have been suffixed with `_lossy`. If you were
    directly using the integer lossy converters, we recommend you use the safe
    converters.
    _Before:_
    ```rust
    fn f1(n: aws_smithy_types::Number) {
        let foo: f32 = n.to_f32(); // Lossy conversion!
        let bar: u32 = n.to_u32(); // Lossy conversion!
    }
    ```
    _After:_
    ```rust
    fn f1(n: aws_smithy_types::Number) {
        use std::convert::TryInto; // Unnecessary import if you're using Rust 2021 edition.
        let foo: f32 = n.try_into().expect("lossy conversion detected"); // Or handle the error instead of panicking.
        // You can still do lossy conversions, but only into floating point types.
        let foo: f32 = n.to_f32_lossy();
        // To lossily convert into integer types, use an `as` cast directly.
        let bar: u32 = n as u32; // Lossy conversion!
    }
    ```
- ⚠ (all, [smithy-rs#1699](https://github.com/awslabs/smithy-rs/issues/1699)) Bump [MSRV](https://github.com/awslabs/aws-sdk-rust#supported-rust-versions-msrv) from 1.58.1 to 1.61.0 per our policy.

**New this release:**
- 🎉 (all, [smithy-rs#1623](https://github.com/awslabs/smithy-rs/issues/1623), @ogudavid) Update Smithy dependency to 1.23.1. Models using version 2.0 of the IDL are now supported.
- 🎉 (server, [smithy-rs#1551](https://github.com/awslabs/smithy-rs/issues/1551), @hugobast) There is a canonical and easier way to run smithy-rs on Lambda [see example].

    [see example]: https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/lambda.rs
- 🐛 (all, [smithy-rs#1623](https://github.com/awslabs/smithy-rs/issues/1623), @ogudavid) Fix detecting sensitive members through their target shape having the @sensitive trait applied.
- (all, [smithy-rs#1623](https://github.com/awslabs/smithy-rs/issues/1623), @ogudavid) Fix SetShape matching needing to occur before ListShape since it is now a subclass. Sets were deprecated in Smithy 1.22.
- (all, [smithy-rs#1623](https://github.com/awslabs/smithy-rs/issues/1623), @ogudavid) Fix Union shape test data having an invalid empty union. Break fixed from Smithy 1.21 to Smithy 1.22.
- (all, [smithy-rs#1612](https://github.com/awslabs/smithy-rs/issues/1612), @unexge) Add codegen version to generated package metadata
- (client, [aws-sdk-rust#609](https://github.com/awslabs/aws-sdk-rust/issues/609)) It is now possible to exempt specific operations from XML body root checking. To do this, add the `AllowInvalidXmlRoot`
    trait to the output struct of the operation you want to exempt.

**Contributors**
Thank you for your contributions! ❤
- @hugobast ([smithy-rs#1551](https://github.com/awslabs/smithy-rs/issues/1551))
- @ogudavid ([smithy-rs#1623](https://github.com/awslabs/smithy-rs/issues/1623))
- @unexge ([smithy-rs#1612](https://github.com/awslabs/smithy-rs/issues/1612))
- @weihanglo ([smithy-rs#1416](https://github.com/awslabs/smithy-rs/issues/1416), [smithy-rs#1635](https://github.com/awslabs/smithy-rs/issues/1635))

August 4th, 2022
================
**Breaking Changes:**
- ⚠🎉 (all, [smithy-rs#1570](https://github.com/awslabs/smithy-rs/issues/1570), @weihanglo) Support @deprecated trait for aggregate shapes
- ⚠ (all, [smithy-rs#1157](https://github.com/awslabs/smithy-rs/issues/1157)) Rename EventStreamInput to EventStreamSender
- ⚠ (all, [smithy-rs#1157](https://github.com/awslabs/smithy-rs/issues/1157)) The type of streaming unions that contain errors is generated without those errors.
    Errors in a streaming union `Union` are generated as members of the type `UnionError`.
    Taking Transcribe as an example, the `AudioStream` streaming union generates, in the client, both the `AudioStream` type:
    ```rust
    pub enum AudioStream {
        AudioEvent(crate::model::AudioEvent),
        Unknown,
    }
    ```
    and its error type,
    ```rust
    pub struct AudioStreamError {
        /// Kind of error that occurred.
        pub kind: AudioStreamErrorKind,
        /// Additional metadata about the error, including error code, message, and request ID.
        pub(crate) meta: aws_smithy_types::Error,
    }
    ```
    `AudioStreamErrorKind` contains all error variants for the union.
    Before, the generated code looked as:
    ```rust
    pub enum AudioStream {
        AudioEvent(crate::model::AudioEvent),
        ... all error variants,
        Unknown,
    }
    ```
- ⚠ (all, [smithy-rs#1157](https://github.com/awslabs/smithy-rs/issues/1157)) `aws_smithy_http::event_stream::EventStreamSender` and `aws_smithy_http::event_stream::Receiver` are now generic over `<T, E>`,
    where `T` is a streaming union and `E` the union's errors.
    This means that event stream errors are now sent as `Err` of the union's error type.
    With this example model:
    ```smithy
    @streaming union Event {
        throttlingError: ThrottlingError
    }
    @error("client") structure ThrottlingError {}
    ```
    Before:
    ```rust
    stream! { yield Ok(Event::ThrottlingError ...) }
    ```
    After:
    ```rust
    stream! { yield Err(EventError::ThrottlingError ...) }
    ```
    An example from the SDK is in [transcribe streaming](https://github.com/awslabs/smithy-rs/blob/4f51dd450ea3234a7faf481c6025597f22f03805/aws/sdk/integration-tests/transcribestreaming/tests/test.rs#L80).

**New this release:**
- 🎉 (all, [smithy-rs#1482](https://github.com/awslabs/smithy-rs/issues/1482)) Update codegen to generate support for flexible checksums.
- (all, [smithy-rs#1520](https://github.com/awslabs/smithy-rs/issues/1520)) Add explicit cast during JSON deserialization in case of custom Symbol providers.
- (all, [smithy-rs#1578](https://github.com/awslabs/smithy-rs/issues/1578), @lkts) Change detailed logs in CredentialsProviderChain from info to debug
- (all, [smithy-rs#1573](https://github.com/awslabs/smithy-rs/issues/1573), [smithy-rs#1569](https://github.com/awslabs/smithy-rs/issues/1569)) Non-streaming struct members are now marked `#[doc(hidden)]` since they will be removed in the future

**Contributors**
Thank you for your contributions! ❤
- @lkts ([smithy-rs#1578](https://github.com/awslabs/smithy-rs/issues/1578))
- @weihanglo ([smithy-rs#1570](https://github.com/awslabs/smithy-rs/issues/1570))

July 20th, 2022
===============
**New this release:**
- 🎉 (all, [aws-sdk-rust#567](https://github.com/awslabs/aws-sdk-rust/issues/567)) Updated the smithy client's retry behavior to allow for a configurable initial backoff. Previously, the initial backoff
    (named `r` in the code) was set to 2 seconds. This is not an ideal default for services that expect clients to quickly
    retry failed request attempts. Now, users can set quicker (or slower) backoffs according to their needs.
- (all, [smithy-rs#1263](https://github.com/awslabs/smithy-rs/issues/1263)) Add checksum calculation and validation wrappers for HTTP bodies.
- (all, [smithy-rs#1263](https://github.com/awslabs/smithy-rs/issues/1263)) `aws_smithy_http::header::append_merge_header_maps`, a function for merging two `HeaderMap`s, is now public.


v0.45.0 (June 28th, 2022)
=========================
**Breaking Changes:**
- ⚠ ([smithy-rs#932](https://github.com/awslabs/smithy-rs/issues/932)) Replaced use of `pin-project` with equivalent `pin-project-lite`. For pinned enum tuple variants and tuple structs, this
    change requires that we switch to using enum struct variants and regular structs. Most of the structs and enums that
    were updated had only private fields/variants and so have the same public API. However, this change does affect the
    public API of `aws_smithy_http_tower::map_request::MapRequestFuture<F, E>`. The `Inner` and `Ready` variants contained a
    single value. Each have been converted to struct variants and the inner value is now accessible by the `inner` field
    instead of the `0` field.

**New this release:**
- 🎉 ([smithy-rs#1411](https://github.com/awslabs/smithy-rs/issues/1411), [smithy-rs#1167](https://github.com/awslabs/smithy-rs/issues/1167)) Upgrade to Gradle 7. This change is not a breaking change, however, users of smithy-rs will need to switch to JDK 17
- 🐛 ([smithy-rs#1505](https://github.com/awslabs/smithy-rs/issues/1505), @kiiadi) Fix issue with codegen on Windows where module names were incorrectly determined from filenames

**Contributors**
Thank you for your contributions! ❤
- @kiiadi ([smithy-rs#1505](https://github.com/awslabs/smithy-rs/issues/1505))
<!-- Do not manually edit this file, use `update-changelogs` -->
v0.44.0 (June 22nd, 2022)
=========================
**New this release:**
- ([smithy-rs#1460](https://github.com/awslabs/smithy-rs/issues/1460)) Fix a potential bug with `ByteStream`'s implementation of `futures_core::stream::Stream` and add helpful error messages
    for users on 32-bit systems that try to stream HTTP bodies larger than 4.29Gb.
- 🐛 ([smithy-rs#1427](https://github.com/awslabs/smithy-rs/issues/1427), [smithy-rs#1465](https://github.com/awslabs/smithy-rs/issues/1465), [smithy-rs#1459](https://github.com/awslabs/smithy-rs/issues/1459)) Fix RustWriter bugs for `rustTemplate` and `docs` utility methods
- 🐛 ([aws-sdk-rust#554](https://github.com/awslabs/aws-sdk-rust/issues/554)) Requests to Route53 that return `ResourceId`s often come with a prefix. When passing those IDs directly into another
    request, the request would fail unless they manually stripped the prefix. Now, when making a request with a prefixed ID,
    the prefix will be stripped automatically.


v0.43.0 (June 9th, 2022)
========================
**New this release:**
- 🎉 ([smithy-rs#1381](https://github.com/awslabs/smithy-rs/issues/1381), @alonlud) Add ability to sign a request with all headers, or to change which headers are excluded from signing
- 🎉 ([smithy-rs#1390](https://github.com/awslabs/smithy-rs/issues/1390)) Add method `ByteStream::into_async_read`. This makes it easy to convert `ByteStream`s into a struct implementing `tokio:io::AsyncRead`. Available on **crate feature** `rt-tokio` only.
- ([smithy-rs#1404](https://github.com/awslabs/smithy-rs/issues/1404), @petrosagg) Add ability to specify a different rust crate name than the one derived from the package name
- ([smithy-rs#1404](https://github.com/awslabs/smithy-rs/issues/1404), @petrosagg) Switch to [RustCrypto](https://github.com/RustCrypto)'s implementation of MD5.

**Contributors**
Thank you for your contributions! ❤
- @alonlud ([smithy-rs#1381](https://github.com/awslabs/smithy-rs/issues/1381))
- @petrosagg ([smithy-rs#1404](https://github.com/awslabs/smithy-rs/issues/1404))

v0.42.0 (May 13th, 2022)
========================
**Breaking Changes:**
- ⚠🎉 ([aws-sdk-rust#494](https://github.com/awslabs/aws-sdk-rust/issues/494), [aws-sdk-rust#519](https://github.com/awslabs/aws-sdk-rust/issues/519)) The `aws_smithy_http::byte_stream::bytestream_util::FsBuilder` has been updated to allow for easier creation of
    multi-part requests.

    - `FsBuilder::offset` is a new method allowing users to specify an offset to start reading a file from.
    - `FsBuilder::file_size` has been reworked into `FsBuilder::length` and is now used to specify the amount of data to read.

    With these two methods, it's now simple to create a `ByteStream` that will read a single "chunk" of a file. The example
    below demonstrates how you could divide a single `File` into consecutive chunks to create multiple `ByteStream`s.

    ```rust
    let example_file_path = Path::new("/example.txt");
    let example_file_size = tokio::fs::metadata(&example_file_path).await.unwrap().len();
    let chunks = 6;
    let chunk_size = file_size / chunks;
    let mut byte_streams = Vec::new();

    for i in 0..chunks {
        let length = if i == chunks - 1 {
            // If we're on the last chunk, the length to read might be less than a whole chunk.
            // We substract the size of all previous chunks from the total file size to get the
            // size of the final chunk.
            file_size - (i * chunk_size)
        } else {
            chunk_size
        };

        let byte_stream = ByteStream::read_from()
            .path(&file_path)
            .offset(i * chunk_size)
            .length(length)
            .build()
            .await?;

        byte_streams.push(byte_stream);
    }

    for chunk in byte_streams {
        // Make requests to a service
    }
    ```

**New this release:**
- ([smithy-rs#1352](https://github.com/awslabs/smithy-rs/issues/1352)) Log a debug event when a retry is going to be peformed
- ([smithy-rs#1332](https://github.com/awslabs/smithy-rs/issues/1332), @82marbag) Update generated crates to Rust 2021

**Contributors**
Thank you for your contributions! ❤
- @82marbag ([smithy-rs#1332](https://github.com/awslabs/smithy-rs/issues/1332))

0.41.0 (April 28th, 2022)
=========================
**Breaking Changes:**
- ⚠ ([smithy-rs#1318](https://github.com/awslabs/smithy-rs/issues/1318)) Bump [MSRV](https://github.com/awslabs/aws-sdk-rust#supported-rust-versions-msrv) from 1.56.1 to 1.58.1 per our "two versions behind" policy.

**New this release:**
- ([smithy-rs#1307](https://github.com/awslabs/smithy-rs/issues/1307)) Add new trait for HTTP body callbacks. This is the first step to enabling us to implement optional checksum verification of requests and responses.
- ([smithy-rs#1330](https://github.com/awslabs/smithy-rs/issues/1330)) Upgrade to Smithy 1.21.0


0.40.2 (April 14th, 2022)
=========================

**Breaking Changes:**
- ⚠ ([aws-sdk-rust#490](https://github.com/awslabs/aws-sdk-rust/issues/490)) Update all runtime crates to [edition 2021](https://blog.rust-lang.org/2021/10/21/Rust-1.56.0.html)

**New this release:**
- ([smithy-rs#1262](https://github.com/awslabs/smithy-rs/issues/1262), @liubin) Fix link to Developer Guide in crate's README.md
- ([smithy-rs#1301](https://github.com/awslabs/smithy-rs/issues/1301), @benesch) Update urlencoding crate to v2.1.0

**Contributors**
Thank you for your contributions! ❤
- @benesch ([smithy-rs#1301](https://github.com/awslabs/smithy-rs/issues/1301))
- @liubin ([smithy-rs#1262](https://github.com/awslabs/smithy-rs/issues/1262))

0.39.0 (March 17, 2022)
=======================
**Breaking Changes:**
- ⚠ ([aws-sdk-rust#406](https://github.com/awslabs/aws-sdk-rust/issues/406)) `aws_types::config::Config` has been renamed to `aws_types:sdk_config::SdkConfig`. This is to better differentiate it
    from service-specific configs like `aws_s3_sdk::Config`. If you were creating shared configs with
    `aws_config::load_from_env()`, then you don't have to do anything. If you were directly referring to a shared config,
    update your `use` statements and `struct` names.

    _Before:_
    ```rust
    use aws_types::config::Config;

    fn main() {
        let config = Config::builder()
        // config builder methods...
        .build()
        .await;
    }
    ```

    _After:_
    ```rust
    use aws_types::SdkConfig;

    fn main() {
        let config = SdkConfig::builder()
        // config builder methods...
        .build()
        .await;
    }
    ```
- ⚠ ([smithy-rs#724](https://github.com/awslabs/smithy-rs/issues/724)) Timeout configuration has been refactored a bit. If you were setting timeouts through environment variables or an AWS
    profile, then you shouldn't need to change anything. Take note, however, that we don't currently support HTTP connect,
    read, write, or TLS negotiation timeouts. If you try to set any of those timeouts in your profile or environment, we'll
    log a warning explaining that those timeouts don't currently do anything.

    If you were using timeouts programmatically,
    you'll need to update your code. In previous versions, timeout configuration was stored in a single `TimeoutConfig`
    struct. In this new version, timeouts have been broken up into several different config structs that are then collected
    in a `timeout::Config` struct. As an example, to get the API per-attempt timeout in previous versions you would access
    it with `<your TimeoutConfig>.api_call_attempt_timeout()` and in this new version you would access it with
    `<your timeout::Config>.api.call_attempt_timeout()`. We also made some unimplemented timeouts inaccessible in order to
    avoid giving users the impression that setting them had an effect. We plan to re-introduce them once they're made
    functional in a future update.

**New this release:**
- ([smithy-rs#1225](https://github.com/awslabs/smithy-rs/issues/1225)) `DynMiddleware` is now `clone`able
- ([smithy-rs#1257](https://github.com/awslabs/smithy-rs/issues/1257)) HTTP request property bag now contains list of desired HTTP versions to use when making requests. This list is not currently used but will be in an upcoming update.


0.38.0 (Februrary 24, 2022)
===========================
**Breaking Changes:**
- ⚠ ([smithy-rs#1197](https://github.com/awslabs/smithy-rs/issues/1197)) `aws_smithy_types::retry::RetryKind` had its `NotRetryable` variant split into `UnretryableFailure` and `Unnecessary`. If you implement the `ClassifyResponse`, then successful responses need to return `Unnecessary`, and failures that shouldn't be retried need to return `UnretryableFailure`.
- ⚠ ([smithy-rs#1209](https://github.com/awslabs/smithy-rs/issues/1209)) `aws_smithy_types::primitive::Encoder` is now a struct rather than an enum, but its usage remains the same.
- ⚠ ([smithy-rs#1217](https://github.com/awslabs/smithy-rs/issues/1217)) `ClientBuilder` helpers `rustls()` and `native_tls()` now return `DynConnector` and use dynamic dispatch rather than returning their concrete connector type that would allow static dispatch. If static dispatch is desired, then manually construct a connector to give to the builder. For example, for rustls: `builder.connector(Adapter::builder().build(aws_smithy_client::conns::https()))` (where `Adapter` is in `aws_smithy_client::hyper_ext`).

**New this release:**
- 🐛 ([smithy-rs#1197](https://github.com/awslabs/smithy-rs/issues/1197)) Fixed a bug that caused clients to eventually stop retrying. The cross-request retry allowance wasn't being reimbursed upon receiving a successful response, so once this allowance reached zero, no further retries would ever be attempted.


0.37.0 (February 18th, 2022)
============================
**Breaking Changes:**
- ⚠ ([smithy-rs#1144](https://github.com/awslabs/smithy-rs/issues/1144)) Some APIs required that timeout configuration be specified with an `aws_smithy_client::timeout::Settings` struct while
    others required an `aws_smithy_types::timeout::TimeoutConfig` struct. Both were equivalent. Now `aws_smithy_types::timeout::TimeoutConfig`
    is used everywhere and `aws_smithy_client::timeout::Settings` has been removed. Here's how to migrate code your code that
    depended on `timeout::Settings`:

    The old way:
    ```rust
    let timeout = timeout::Settings::new()
        .with_connect_timeout(Duration::from_secs(1))
        .with_read_timeout(Duration::from_secs(2));
    ```

    The new way:
    ```rust
    // This example is passing values, so they're wrapped in `Option::Some`. You can disable a timeout by passing `None`.
    let timeout = TimeoutConfig::new()
        .with_connect_timeout(Some(Duration::from_secs(1)))
        .with_read_timeout(Some(Duration::from_secs(2)));
    ```
- ⚠ ([smithy-rs#1085](https://github.com/awslabs/smithy-rs/issues/1085)) Moved the following re-exports into a `types` module for all services:
    - `<service>::AggregatedBytes` -> `<service>::types::AggregatedBytes`
    - `<service>::Blob` -> `<service>::types::Blob`
    - `<service>::ByteStream` -> `<service>::types::ByteStream`
    - `<service>::DateTime` -> `<service>::types::DateTime`
    - `<service>::SdkError` -> `<service>::types::SdkError`
- ⚠ ([smithy-rs#1085](https://github.com/awslabs/smithy-rs/issues/1085)) `AggregatedBytes` and `ByteStream` are now only re-exported if the service has streaming operations,
    and `Blob`/`DateTime` are only re-exported if the service uses them.
- ⚠ ([smithy-rs#1130](https://github.com/awslabs/smithy-rs/issues/1130)) MSRV increased from `1.54` to `1.56.1` per our 2-behind MSRV policy.

**New this release:**
- ([smithy-rs#1144](https://github.com/awslabs/smithy-rs/issues/1144)) `MakeConnectorFn`, `HttpConnector`, and `HttpSettings` have been moved from `aws_config::provider_config` to
    `aws_smithy_client::http_connector`. This is in preparation for a later update that will change how connectors are
    created and configured.
- ([smithy-rs#1123](https://github.com/awslabs/smithy-rs/issues/1123)) Refactor `Document` shape parser generation
- ([smithy-rs#1085](https://github.com/awslabs/smithy-rs/issues/1085)) The `Client` and `Config` re-exports now have their documentation inlined in the service docs


0.36.0 (January 26, 2022)
=========================
**New this release:**
- ([smithy-rs#1087](https://github.com/awslabs/smithy-rs/issues/1087)) Improve docs on `Endpoint::{mutable, immutable}`
- ([smithy-rs#1118](https://github.com/awslabs/smithy-rs/issues/1118)) SDK examples now come from [`awsdocs/aws-doc-sdk-examples`](https://github.com/awsdocs/aws-doc-sdk-examples) rather than from `smithy-rs`
- ([smithy-rs#1114](https://github.com/awslabs/smithy-rs/issues/1114), @mchoicpe-amazon) Provide SigningService creation via owned String

**Contributors**
Thank you for your contributions! ❤
- @mchoicpe-amazon ([smithy-rs#1114](https://github.com/awslabs/smithy-rs/issues/1114))


0.35.2 (January 20th, 2022)
===========================
_Changes only impact generated AWS SDK_

v0.35.1 (January 19th, 2022)
============================
_Changes only impact generated AWS SDK_


0.35.0 (January 19, 2022)
=========================
**New this release:**
- ([smithy-rs#1053](https://github.com/awslabs/smithy-rs/issues/1053)) Upgraded Smithy to 1.16.1
- 🐛 ([smithy-rs#1069](https://github.com/awslabs/smithy-rs/issues/1069)) Fix broken link to `RetryMode` in client docs
- 🐛 ([smithy-rs#1069](https://github.com/awslabs/smithy-rs/issues/1069)) Fix several doc links to raw identifiers (identifiers excaped with `r#`)
- 🐛 ([smithy-rs#1069](https://github.com/awslabs/smithy-rs/issues/1069)) Reduce dependency recompilation in local dev
- 🐛 ([aws-sdk-rust#405](https://github.com/awslabs/aws-sdk-rust/issues/405), [smithy-rs#1083](https://github.com/awslabs/smithy-rs/issues/1083)) Fixed paginator bug impacting EC2 describe VPCs (and others)



v0.34.1 (January 10, 2022)
==========================
**New this release:**
- 🐛 (smithy-rs#1054, aws-sdk-rust#391) Fix critical paginator bug where an empty outputToken lead to a never ending stream.



0.34.0 (January 6th, 2022)
==========================
**Breaking Changes:**
- ⚠ (smithy-rs#990) Codegen will no longer produce builders and clients with methods that take `impl Into<T>` except for strings and boxed types.
- ⚠ (smithy-rs#1003) The signature of `aws_smithy_protocol_test::validate_headers` was made more flexible but may require adjusting invocations slightly.

**New this release:**
- 🎉 (aws-sdk-rust#47, smithy-rs#1006) Add support for paginators! Paginated APIs now include `.into_paginator()` and (when supported) `.into_paginator().items()` to enable paginating responses automatically. The paginator API should be considered in preview and is subject to change pending customer feedback.
- 🐛 (aws-sdk-rust#357) Generated docs will convert `<a>` tags with no `href` attribute to `<pre>` tags
- (aws-sdk-rust#254, @jacco) Made fluent operation structs cloneable

**Contributors**
Thank you for your contributions! ❤
- @jacco (aws-sdk-rust#254)


v0.33.1 (December 15th, 2021)
=============================
**New this release:**
- 🐛 (smithy-rs#979) Make `aws-smithy-client` a required dependency in generated services.



v0.33.0 (December 15th, 2021)
=============================
**Breaking Changes:**
- ⚠ (smithy-rs#930) Runtime crates no longer have default features. You must now specify the features that you want when you add a dependency to your `Cargo.toml`.

    **Upgrade guide**

    | before                          | after |
    |---------------------------------|-------|
    | `aws-smithy-async = "VERSION"`  | `aws-smithy-async = { version = "VERSION", features = ["rt-tokio"] }` |
    | `aws-smithy-client = "VERSION"` | `aws-smithy-client = { version = "VERSION", features = ["client-hyper", "rustls", "rt-tokio"] }` |
    | `aws-smithy-http = "VERSION"`   | `aws-smithy-http = { version = "VERSION", features = ["rt-tokio"] }` |
- ⚠ (smithy-rs#940) `aws_smithy_client::Client::https()` has been renamed to `dyn_https()`.
    This is to clearly distinguish it from `rustls` and `native_tls` which do not use a boxed connector.

**New this release:**
- 🐛 (smithy-rs#957) Include non-service-specific examples in the generated root Cargo workspace
- 🎉 (smithy-rs#922, smithy-rs#914) Add changelog automation to sdk-lints
- 🐛 (aws-sdk-rust#317, smithy-rs#907) Removed spamming log message when a client was used without a sleep implementation, and
    improved context and call to action in logged messages around missing sleep implementations.
- (smithy-rs#923) Use provided `sleep_impl` for retries instead of using Tokio directly.
- (smithy-rs#920) Fix typos in module documentation for generated crates
- 🐛 (aws-sdk-rust#301, smithy-rs#892) Avoid serializing repetitive `xmlns` attributes in generated XML serializers.
- 🐛 (smithy-rs#953, aws-sdk-rust#331) Fixed a bug where certain characters caused a panic during URI encoding.



v0.32.0 (December 2nd, 2021)
=======================

- This release was a version bump to fix a version number conflict in crates.io

v0.31.0 (December 2nd, 2021)
=======================
**New this week**
- Add docs.rs metadata section to all crates to document all features


v0.30.0-alpha (November 23rd, 2021)
===================================

**New this week**
- Improve docs on `aws-smithy-client` (smithy-rs#855)
- Fix http-body dependency version (smithy-rs#883, aws-sdk-rust#305)
- `SdkError` now includes a variant `TimeoutError` for when a request times out (smithy-rs#885)
- Timeouts for requests are now configurable. You can set separate timeouts for each individual request attempt and all attempts made for a request. (smithy-rs#831)

**Breaking Changes**
- (aws-smithy-client): Extraneous `pub use SdkSuccess` removed from `aws_smithy_client::hyper_ext`. (smithy-rs#855)


v0.29.0-alpha (November 11th, 2021)
===================================

**Breaking Changes**

Several breaking changes around `aws_smithy_types::Instant` were introduced by smithy-rs#849:
- `aws_smithy_types::Instant` from was renamed to `DateTime` to avoid confusion with the standard library's monotonically non-decreasing `Instant` type.
- `DateParseError` in `aws_smithy_types` has been renamed to `DateTimeParseError` to match the type that's being parsed.
- The `chrono-conversions` feature and associated functions have been moved to the `aws-smithy-types-convert` crate.
  - Calls to `Instant::from_chrono` should be changed to:
    ```rust
    use aws_smithy_types::DateTime;
    use aws_smithy_types_convert::date_time::DateTimeExt;

    // For chrono::DateTime<Utc>
    let date_time = DateTime::from_chrono_utc(chrono_date_time);
    // For chrono::DateTime<FixedOffset>
    let date_time = DateTime::from_chrono_offset(chrono_date_time);
    ```
  - Calls to `instant.to_chrono()` should be changed to:
    ```rust
    use aws_smithy_types_convert::date_time::DateTimeExt;

    date_time.to_chrono_utc();
    ```
- `Instant::from_system_time` and `Instant::to_system_time` have been changed to `From` trait implementations.
  - Calls to `from_system_time` should be changed to:
    ```rust
    DateTime::from(system_time);
    // or
    let date_time: DateTime = system_time.into();
    ```
  - Calls to `to_system_time` should be changed to:
    ```rust
    SystemTime::from(date_time);
    // or
    let system_time: SystemTime = date_time.into();
    ```
- Several functions in `Instant`/`DateTime` were renamed:
  - `Instant::from_f64` -> `DateTime::from_secs_f64`
  - `Instant::from_fractional_seconds` -> `DateTime::from_fractional_secs`
  - `Instant::from_epoch_seconds` -> `DateTime::from_secs`
  - `Instant::from_epoch_millis` -> `DateTime::from_millis`
  - `Instant::epoch_fractional_seconds` -> `DateTime::as_secs_f64`
  - `Instant::has_nanos` -> `DateTime::has_subsec_nanos`
  - `Instant::epoch_seconds` -> `DateTime::secs`
  - `Instant::epoch_subsecond_nanos` -> `DateTime::subsec_nanos`
  - `Instant::to_epoch_millis` -> `DateTime::to_millis`
- The `DateTime::fmt` method is now fallible and fails when a `DateTime`'s value is outside what can be represented by the desired date format.
- In `aws-sigv4`, the `SigningParams` builder's `date_time` setter was renamed to `time` and changed to take a `std::time::SystemTime` instead of a chrono's `DateTime<Utc>`.

**New this week**

- :warning: MSRV increased from 1.53.0 to 1.54.0 per our 3-behind MSRV policy.
- Conversions from `aws_smithy_types::DateTime` to `OffsetDateTime` from the `time` crate are now available from the `aws-smithy-types-convert` crate. (smithy-rs#849)
- Fixed links to Usage Examples (smithy-rs#862, @floric)

v0.28.0-alpha (November 11th, 2021)
===================================

No changes since last release except for version bumping since older versions
of the AWS SDK were failing to compile with the `0.27.0-alpha.2` version chosen
for the previous release.

v0.27.0-alpha.2 (November 9th, 2021)
=======================
**Breaking Changes**

- Members named `builder` on model structs were renamed to `builder_value` so that their accessors don't conflict with the existing `builder()` methods (smithy-rs#842)

**New this week**

- Fix epoch seconds date-time parsing bug in `aws-smithy-types` (smithy-rs#834)
- Omit trailing zeros from fraction when formatting HTTP dates in `aws-smithy-types` (smithy-rs#834)
- Generated structs now have accessor methods for their members (smithy-rs#842)

v0.27.0-alpha.1 (November 3rd, 2021)
====================================
**Breaking Changes**
- `<operation>.make_operation(&config)` is now an `async` function for all operations. Code should be updated to call `.await`. This will only impact users using the low-level API. (smithy-rs#797)

**New this week**
- SDK code generation now includes a version in addition to path parameters when the `version` parameter is included in smithy-build.json
- `moduleDescription` in `smithy-build.json` settings is now optional
- Upgrade to Smithy 1.12
- `hyper::Error(IncompleteMessage)` will now be retried (smithy-rs#815)
- Unions will optionally generate an `Unknown` variant to support parsing variants that don't exist on the client. These variants will fail to serialize if they are ever included in requests.
- Fix generated docs on unions. (smithy-rs#826)

v0.27 (October 20th, 2021)
==========================

**Breaking Changes**

- :warning: All Smithy runtime crates have been renamed to have an `aws-` prefix. This may require code changes:
  - _Cargo.toml_ changes:
    - `smithy-async` -> `aws-smithy-async`
    - `smithy-client` -> `aws-smithy-client`
    - `smithy-eventstream` -> `aws-smithy-eventstream`
    - `smithy-http` -> `aws-smithy-http`
    - `smithy-http-tower` -> `aws-smithy-http-tower`
    - `smithy-json` -> `aws-smithy-json`
    - `smithy-protocol-test` -> `aws-smithy-protocol-test`
    - `smithy-query` -> `aws-smithy-query`
    - `smithy-types` -> `aws-smithy-types`
    - `smithy-xml` -> `aws-smithy-xml`
  - Rust `use` statement changes:
    - `smithy_async` -> `aws_smithy_async`
    - `smithy_client` -> `aws_smithy_client`
    - `smithy_eventstream` -> `aws_smithy_eventstream`
    - `smithy_http` -> `aws_smithy_http`
    - `smithy_http_tower` -> `aws_smithy_http_tower`
    - `smithy_json` -> `aws_smithy_json`
    - `smithy_protocol_test` -> `aws_smithy_protocol_test`
    - `smithy_query` -> `aws_smithy_query`
    - `smithy_types` -> `aws_smithy_types`
    - `smithy_xml` -> `aws_smithy_xml`

**New this week**

- Filled in missing docs for services in the rustdoc documentation (smithy-rs#779)

v0.26 (October 15th, 2021)
=======================

**Breaking Changes**

- :warning: The `rust-codegen` plugin now requires a `moduleDescription` in the *smithy-build.json* file. This
  property goes into the generated *Cargo.toml* file as the package description. (smithy-rs#766)

**New this week**

- Add `RustSettings` to `CodegenContext` (smithy-rs#616, smithy-rs#752)
- Prepare crate manifests for publishing to crates.io (smithy-rs#755)
- Generated *Cargo.toml* files can now be customized (smithy-rs#766)

v0.25.1 (October 11th, 2021)
=========================
**New this week**
- :bug: Re-add missing deserialization operations that were missing because of a typo in `HttpBoundProtocolGenerator.kt`

v0.25 (October 7th, 2021)
=========================
**Breaking changes**
- :warning: MSRV increased from 1.52.1 to 1.53.0 per our 3-behind MSRV policy.
- :warning: `smithy_client::retry::Config` field `max_retries` is renamed to `max_attempts`
  - This also brings a change to the semantics of the field. In the old version, setting `max_retries` to 3 would mean
    that up to 4 requests could occur (1 initial request and 3 retries). In the new version, setting `max_attempts` to 3
    would mean that up to 3 requests could occur (1 initial request and 2 retries).
- :warning: `smithy_client::retry::Config::with_max_retries` method is renamed to `with_max_attempts`
- :warning: Several classes in the codegen module were renamed and/or refactored (smithy-rs#735):
  - `ProtocolConfig` became `CodegenContext` and moved to `software.amazon.smithy.rust.codegen.smithy`
  - `HttpProtocolGenerator` became `ProtocolGenerator` and was refactored
    to rely on composition instead of inheritance
  - `HttpProtocolTestGenerator` became `ProtocolTestGenerator`
  - `Protocol` moved into `software.amazon.smithy.rust.codegen.smithy.protocols`
- `SmithyConnector` and `DynConnector` now return `ConnectorError` instead of `Box<dyn Error>`. If you have written a custom connector, it will need to be updated to return the new error type. (#744)
- The `DispatchError` variant of `SdkError` now contains `ConnectorError` instead of `Box<dyn Error>` (#744).

**New this week**

- :bug: Fix an issue where `smithy-xml` may have generated invalid XML (smithy-rs#719)
- Add `RetryConfig` struct for configuring retry behavior (smithy-rs#725)
- :bug: Fix error when receiving empty event stream messages (smithy-rs#736)
- :bug: Fix bug in event stream receiver that could cause the last events in the response stream to be lost (smithy-rs#736)
- Add connect & HTTP read timeouts to IMDS, defaulting to 1 second
- IO and timeout errors from Hyper can now be retried (#744)

**Contributors**

Thank you for your contributions! :heart:
* @obi1kenobi (smithy-rs#719)
* @guyilin-amazon (smithy-rs#750)

v0.24 (September 24th, 2021)
============================

**New This Week**

- Add IMDS credential provider to `aws-config` (smithy-rs#709)
- Add IMDS client to `aws-config` (smithy-rs#701)
- Add `TimeSource` to `aws_types::os_shim_internal` (smithy-rs#701)
- User agent construction is now `const fn` (smithy-rs#701)
- Add `sts::AssumeRoleProvider` to `aws-config` (smithy-rs#703, aws-sdk-rust#3)
- Add IMDS region provider to `aws-config` (smithy-rs#715)
- Add query param signing to the `aws-sigv4` crate (smithy-rs#707)
- :bug: Update event stream `Receiver`s to be `Send` (smithy-rs#702, #aws-sdk-rust#224)

v0.23 (September 14th, 2021)
=======================

**New This Week**
- :bug: Fixes issue where `Content-Length` header could be duplicated leading to signing failure (aws-sdk-rust#220, smithy-rs#697)
- :bug: Fixes naming collision during generation of model shapes that collide with `<operationname>Input` and `<operationname>Output` (#699)

v0.22 (September 2nd, 2021)
===========================

This release adds support for three commonly requested features:
- More powerful credential chain
- Support for constructing multiple clients from the same configuration
- Support for Transcribe streaming and S3 Select

In addition, this overhauls client configuration which lead to a number of breaking changes. Detailed changes are inline.

Current Credential Provider Support:
- [x] Environment variables
- [x] Web Identity Token Credentials
- [ ] Profile file support (partial)
  - [ ] Credentials
    - [ ] SSO
    - [ ] ECS Credential source
    - [ ] IMDS credential source
    - [x] Assume role from source profile
    - [x] Static credentials source profile
    - [x] WebTokenIdentity provider
  - [x] Region
- [ ] IMDS
- [ ] ECS

Upgrade Guide
-------------

### If you use `<sdk>::Client::from_env`

`from_env` loaded region & credentials from environment variables _only_. Default sources have been removed from the generated
SDK clients and moved to the `aws-config` package. Note that the `aws-config` package default chain adds support for
profile file and web identity token profiles.

1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```
2. Update your client creation code:
   ```rust
   // `shared_config` can be used to construct multiple different service clients!
   let shared_config = aws_config::load_from_env().await;
   // before: <service>::Client::from_env();
   let client = <service>::Client::new(&shared_config)
   ```

### If you used `<client>::Config::builder()`

`Config::build()` has been modified to _not_ fallback to a default provider. Instead, use `aws-config` to load and modify
the default chain. Note that when you switch to `aws-config`, support for profile files and web identity tokens will be added.

1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```

2. Update your client creation code:

   ```rust
   fn before() {
     let region = aws_types::region::ChainProvider::first_try(<1 provider>).or_default_provider();
     let config = <service>::Config::builder().region(region).build();
     let client = <service>::Client::from_conf(&config);
   }

   async fn after() {
     use aws_config::meta::region::RegionProviderChain;
     let region_provider = RegionProviderChain::first_try(<1 provider>).or_default_provider();
     // `shared_config` can be used to construct multiple different service clients!
     let shared_config = aws_config::from_env().region(region_provider).load().await;
     let client = <service>::Client::new(&shared_config)
   }
   ```

### If you used `aws-auth-providers`
All credential providers that were in `aws-auth-providers` have been moved to `aws-config`. Unless you have a specific use case
for a specific credential provider, you should use the default provider chain:

```rust
 let shared_config = aws_config::load_from_env().await;
 let client = <service>::Client::new(&shared_config);
```

### If you maintain your own credential provider

`AsyncProvideCredentials` has been renamed to `ProvideCredentials`. The trait has been moved from `aws-auth` to `aws-types`.
The original `ProvideCredentials` trait has been removed. The return type has been changed to by a custom future.

For synchronous use cases:
```rust
use aws_types::credentials::{ProvideCredentials, future};

#[derive(Debug)]
struct CustomCreds;
impl ProvideCredentials for CustomCreds {
  fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
            Self: 'a,
  {
    // if your credentials are synchronous, use `::ready`
    // if your credentials are loaded asynchronously, use `::new`
    future::ProvideCredentials::ready(todo!()) // your credentials go here
  }
}
```

For asynchronous use cases:
```rust
use aws_types::credentials::{ProvideCredentials, future, Result};

#[derive(Debug)]
struct CustomAsyncCreds;
impl CustomAsyncCreds {
  async fn load_credentials(&self) -> Result {
    Ok(Credentials::from_keys("my creds...", "secret", None))
  }
}

impl ProvideCredentials for CustomCreds {
  fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
            Self: 'a,
  {
    future::ProvideCredentials::new(self.load_credentials())
  }
}
```

Changes
-------

**Breaking Changes**

- Credential providers from `aws-auth-providers` have been moved to `aws-config` (#678)
- `AsyncProvideCredentials` has been renamed to `ProvideCredentials`. The original non-async provide credentials has been
  removed. See the migration guide above.
- `<sevicename>::from_env()` has been removed (#675). A drop-in replacement is available:
  1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```
  2. Update your client creation code:
     ```rust
     let client = <service>>::Client::new(&aws_config::load_from_env().await)
     ```

- `ProvideRegion` has been moved to `aws_config::meta::region::ProvideRegion`. (#675)
- `aws_types::region::ChainProvider` has been moved to `aws_config::meta::region::RegionProviderChain` (#675).
- `ProvideRegion` is now asynchronous. Code that called `provider.region()` must be changed to `provider.region().await`.
- `<awsservice>::Config::builder()` will **not** load a default region. To preserve previous behavior:
  1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```
  2. ```rust
     let shared_config = aws_config::load_from_env().await;
     let config = <service>::config::Builder::from(&shared_config).<other builder modifications>.build();
     ```
- `Request` and `Response` in `smithy_http::operation` now use `SharedPropertyBag` instead of `Arc<Mutex<PropertyBag>>`. Use the `acquire` and `acquire_mut` methods to get a reference to the underlying `PropertyBag` to access properties. (#667)

**New this week**

- :tada: Add profile file provider for region (#594, #682)
- :tada: Add support for shared configuration between multiple services (#673)
- :tada: Add support for Transcribe `StartStreamTranscription` and S3 `SelectObjectContent` operations (#667)
- :tada: Add support for new MemoryDB service (#677)
- Improve documentation on collection-aware builders (#664)
- Update AWS SDK models (#677)
- :bug: Fix sigv4 signing when request ALPN negotiates to HTTP/2. (#674)
- :bug: Fix integer size on S3 `Size` (#679, aws-sdk-rust#209)
- :bug: Fix JSON parsing issue for modeled empty structs (#683, aws-sdk-rust#212)
- :bug: Fix acronym case disagreement between FluentClientGenerator and HttpProtocolGenerator type aliasing (#668)

**Internal Changes**

- Add Event Stream support for restJson1 and restXml (#653, #667)
- Add NowOrLater future to smithy-async (#672)


v0.21 (August 19th, 2021)
=========================

**New This Week**

- :tada: Add Chime Identity, Chime Messaging, and Snow Device Management support (#657)
- :tada: Add profile file credential provider implementation. This implementation currently does not support credential sources for assume role providers other than environment variables. (#640)
- :tada: Add support for WebIdentityToken providers via profile & environment variables. (#654)
- :bug: Fix name collision that occurred when a model had both a union and a structure named `Result` (#643)
- :bug: Fix STS Assume Role with WebIdentity & Assume role with SAML to support clients with no credentials provided (#652)
- Update AWS SDK models (#657)
- Add initial implementation of a default provider chain. (#650)

**Internal Changes**

- Update sigv4 tests to work around behavior change in httparse 1.5. (#656)
- Remove Bintray/JCenter source from gradle build. (#651)
- Add experimental `dvr` module to smithy-client. This will enable easier testing of HTTP traffic. (#640)
- Update smithy-client to simplify creating HTTP/HTTPS connectors (#650)
- Add Event Stream support to aws-sigv4 (#648)
- Add support for the smithy auth trait. This enables authorizations that explicitly disable authorization to work when no credentials have been provided. (#652)

v0.20 (August 10th, 2021)
=========================

**Breaking changes**

- (#635) The `config()`, `config_mut()`, `request()`, and `request_mut()` methods on `operation::Request` have been
  renamed to `properties()`, `properties_mut()`, `http()`, and `http_mut()` respectively.
- (#635) The `Response` type on Tower middleware has been changed from `http::Response<SdkBody>`
  to `operation::Response`. The HTTP response is still available from the `operation::Response` using its `http()`
  and `http_mut()` methods.
- (#635) The `ParseHttpResponse` trait's `parse_unloaded()` method now takes an `operation::Response` rather than
  an `http::Response<SdkBody>`.
- (#626) `ParseHttpResponse` no longer has a generic argument for the body type, but instead, always uses `SdkBody`.
  This may cause compilation failures for you if you are using Smithy generated types to parse JSON or XML without using
  a client to request data from a service. The fix should be as simple as removing `<SdkBody>` in the example below:

  Before:
  ```rust
  let output = <Query as ParseHttpResponse<SdkBody>>::parse_loaded(&parser, &response).unwrap();
  ```

  After:
  ```rust
  let output = <Query as ParseHttpResponse>::parse_loaded(&parser, &response).unwrap();
  ```

**New This Week**

- Add AssumeRoleProvider parser implementation. (#632)
- The closure passed to `provide_credentials_fn` can now borrow values (#637)
- Add `Sender`/`Receiver` implementations for Event Stream (#639)
- Bring in the latest AWS models (#630)

v0.19 (August 3rd, 2021)
========================

IoT Data Plane is now available! If you discover it isn't functioning as expected, please let us know!

This week also sees the addition of a robust async caching credentials provider. Take a look at the
[STS example](https://github.com/awslabs/smithy-rs/blob/7fa4af4a9367aeca6d55e26fc4d4ba93093b90c4/aws/sdk/examples/sts/src/bin/credentials-provider.rs)
to see how to use it.

**New This Week**

- :tada: Add IoT Data Plane (#624)
- :tada: Add LazyCachingCredentialsProvider to aws-auth for use with expiring credentials, such as STS AssumeRole.
  Update STS example to use this new provider (#578, #595)
- :bug: Correctly encode HTTP Checksums using base64 instead of hex. Fixes aws-sdk-rust#164. (#615)
- Update SDK gradle build logic to use gradle properties (#620)
- Overhaul serialization/deserialization of numeric/boolean types. This resolves issues around serialization of
  NaN/Infinity and should also reduce the number of allocations required during serialization. (#618)
- Update SQS example to clarify usage of FIFO vs. standard queues (#622, @trevorrobertsjr)
- Implement Event Stream frame encoding/decoding (#609, #619)

**Contributions**

Thank you for your contributions! :heart:

- @trevorrobertsjr (#622)

v0.18.1 (July 27th 2021)
========================

- Remove timestreamwrite and timestreamquery from the generated services (#613)

v0.18 (July 27th 2021)
======================

**Breaking changes**

- `test-util` has been made an optional dependency and has moved from aws-hyper to smithy-http. If you were relying
  on `aws_hyper::TestConnection`, add `smithy-client` as a dependency and enable the optional `test-util` feature. This
  prunes some unnecessary dependencies on `roxmltree` and `serde_json`
  for most users. (#608)

**New This Week**

- :tada: Release all but three remaining AWS services! Glacier, IoT Data Plane and Transcribe streaming will be
  available in a future release. If you discover that a service isn't functioning as expected please let us know! (#607)
- :bug: Bugfix: Fix parsing bug where parsing XML incorrectly stripped whitespace (#590, aws-sdk-rust#153)
- Establish common abstraction for environment variables (#594)
- Add windows to the test matrix (#594)
- :bug: Bugfix: Constrain RFC-3339 timestamp formatting to microsecond precision (#596)

v0.17 (July 15th 2021)
======================

**New this Week**

- :tada: Add support for Autoscaling (#576, #582)
- `AsyncProvideCredentials` now introduces an additional lifetime parameter, simplifying bridging it
  with `#[async_trait]` interfaces
- Fix S3 bug when content type was set explicitly (aws-sdk-rust#131, #566, @eagletmt)

**Contributions**

Thank you for your contributions! :heart:

- @eagletmt (#566)

v0.16 (July 6th 2021)
=====================

**New this Week**

- :warning: **Breaking Change:** `ProvideCredentials` and `CredentialError` were both moved into `aws_auth::provider`
  when they were previously in `aws_auth` (#572)
- :tada: Add support for AWS Config (#570)
- :tada: Add support for EBS (#567)
- :tada: Add support for Cognito (#573)
- :tada: Add support for Snowball (#579, @landonxjames)
- Make it possible to asynchronously provide credentials with `provide_credentials_fn` (#572, #577)
- Improve RDS, QLDB, Polly, and KMS examples (#561, #560, #558, #556, #550)
- Update AWS SDK models (#575)
- :bug: Bugfix: Fill in message from error response even when it doesn't match the modeled case format (#565)

**Internal Changes**

- Add support for `@unsignedPayload` Smithy trait (#567)
- Strip service/api/client suffix from sdkId (#546)
- Remove idempotency token trait (#571)

**Contributions**

Thank you for your contributions! :heart:

- landonxjames (#579)

v0.15 (June 29th 2021)
======================

This week, we've added EKS, ECR and Cloudwatch. The JSON deserialization implementation has been replaced, please be on
the lookout for potential issues.

**New this Week**

- :tada: Add support for ECR (#557)
- :tada: Add support for Cloudwatch (#554)
- :tada: Add support for EKS (#553)
- :warn: **Breaking Change:** httpLabel no longer causes fields to be non-optional. (#537)
- :warn: **Breaking Change:** `Exception` is not renamed to `Error`. Code may need to be updated to replace `exception`
  with `error`
- Add more SES examples, and improve examples for Batch.
- Improved error handling ergonomics: Errors now provide `is_<variantname>()` methods to simplify error handling
- :bug: Bugfix: fix bug where invalid query strings could be generated (#531, @eagletmt)

**Internal Changes**

- Pin CI version to 1.52.1 (#532)
- New JSON deserializer implementation (#530)
- Fix numerous namespace collision bugs (#539)
- Gracefully handle empty response bodies during JSON parsing (#553)

**Contributors**

Thank you for your contributions! :heart:

- @eagletmt (#531)

v0.14 (June 22nd 2021)
======================

This week, we've added CloudWatch Logs support and fixed several bugs in the generated S3 clients. There are a few
breaking changes this week.

**New this Week**

- :tada: Add support for CloudWatch Logs (#526)
- :warning: **Breaking Change:** The `set_*` functions on generated Builders now always take an `Option` (#506)
- :warning: **Breaking Change:** Unions with Documents will see the inner document type change from `Option<Document>`
  to `Document` (#520)
- :warning: **Breaking Change:** The `as_*` functions on unions now return `Result` rather than `Option` to clearly
  indicate what the actual value is (#527)
- Add more S3 examples, and improve SNS, SQS, and SageMaker examples. Improve example doc comments (#490, #508, #509,
  #510, #511, #512, #513, #524)
- :bug: Bugfix: Show response body in trace logs for calls that don't return a stream (#514)
- :bug: Bugfix: Correctly parse S3's GetBucketLocation response (#516)
- :bug: Bugfix: Correctly URL-encode tilde characters before SigV4 signing (#519)
- :bug: Bugfix: Fix S3 PutBucketLifecycle operation by adding support for the `@httpChecksumRequired` Smithy trait (
  #523)
- :bug: Bugfix: Correctly parse non-list headers with commas in them (#525, @eagletmt)

**Internal Changes**

- Reduce name collisions in generated code (#502)
- Combine individual example packages into per-service example packages with multiple binaries (#481, #490)
- Re-export HyperAdapter in smithy-client (#515, @zekisherif)
- Add serialization/deserialization benchmark for DynamoDB to exercise restJson1 generated code (#507)

**Contributions**

Thank you for your contributions! :heart:

- @eagletmt (#525)
- @zekisherif (#515)

v0.13 (June 15th 2021)
======================

Smithy-rs now has codegen support for all AWS services! This week, we've added CloudFormation, SageMaker, EC2, and SES.
More details below.

**New this Week**

- :tada: Add support for CloudFormation (#500, @alistaim)
- :tada: Add support for SageMaker (#473, @alistaim)
- :tada: Add support for EC2 (#495)
- :tada: Add support for SES (#499)
- Add support for the EC2 Query protocol (#475)
- Generate fluent builders for all smithy-rs clients (#496, @jonhoo)
- :bug: Bugfix: RFC-3339 timestamps (`date-time` format in Smithy) are now formatted correctly (#479, #489)
- :bug: Bugfix: Union and enum variants named Self no longer cause compile errors in generated code (#492)

**Internal Changes**

- Combine individual example packages into per-service example packages with multiple binaries (#477, #480, #482, #484,
  #485, #486, #487, #491)
- Work towards JSON deserialization overhaul (#474)
- Make deserializer function naming consistent between XML and JSON deserializers (#497)

Contributors:

- @Doug-AWS
- @jdisanti
- @rcoh
- @alistaim
- @jonhoo

Thanks!!

v0.12 (June 8th 2021)
=====================

Starting this week, smithy-rs now has codegen support for all AWS services except EC2. This week we’ve added MediaLive,
MediaPackage, SNS, Batch, STS, RDS, RDSData, Route53, and IAM. More details below.

**New this Week**

- :tada: Add support for MediaLive and MediaPackage (#449, @alastaim)
- :tada: Add support for SNS (#450)
- :tada: Add support for Batch (#452, @alistaim)
- :tada: Add support for STS. **Note:** This does not include support for an STS-based credential provider although an
  example is provided. (#453)
- :tada: Add support for RDS (#455) and RDS-Data (#470). (@LMJW)
- :tada: Add support for Route53 (#457, @alistaim)
- Support AWS Endpoints & Regions. With this update, regions like `iam-fips` and `cn-north-1` will now resolve to the
  correct endpoint. Please report any issues with endpoint resolution. (#468)
- :bug: Bugfix: Primitive numerics and booleans are now filtered from serialization when they are 0 and not marked as
  required. This resolves issues where maxResults needed to be set even though it is optional. (#451)
- :bug: Bugfix: S3 Head Object returned the wrong error when the object did not exist (#460, fixes #456)

**Internal Changes**

- Remove unused key “build” from smithy-build.json and Rust settings (#447)
- Split SDK CI jobs for faster builds & reporting (#446)
- Fix broken doc link in JSON serializer (@LMJW)
- Work towards JSON deserialization overhaul (#454, #462)

Contributors:

- @rcoh
- @jdisanti
- @alistaim
- @LMJW

Thanks!!

v0.11 (June 1st, 2021)
======================

**New this week:**

- :tada: Add support for SQS. SQS is our first service to use the awsQuery protocol. Please report any issues you may
  encounter.
- :tada: Add support for ECS.
- **Breaking Change**: Refactored `smithy_types::Error` to be more flexible. Internal fields of `Error` are now private
  and can now be accessed accessor functions. (#426)
- `ByteStream::from_path` now accepts `implications AsRef<Path>` (@LMJW)
- Add support for S3 extended request id (#429)
- Add support for the awsQuery protocol. smithy-rs can now add support for all services except EC2.
- **Bugfix**: Timestamps that fell precisely on minute boundaries were not properly formatted (#435)
- Improve documentation for `ByteStream` & add `pub use` (#443)
- Add support for `EndpointPrefix` used
  by [`s3::WriteGetObjectResponse`](https://awslabs.github.io/aws-sdk-rust/aws_sdk_s3/operation/struct.WriteGetObjectResponse.html) (
  #420)

**Smithy Internals**

- Rewrite JSON serializer (#411, #423, #416, #427)
- Remove dead “rootProject” setting in `smithy-build.json`
- **Bugfix:** Idempotency tokens were not properly generated when operations were used by resources

Contributors:

- @jdisanti
- @rcoh
- @LMJW

Thanks!
