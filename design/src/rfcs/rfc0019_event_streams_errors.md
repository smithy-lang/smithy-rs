<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Errors for event streams
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: Implemented

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines how client and server will use errors defined in `@streaming` unions (event streams).

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

In the current version of smithy-rs, customers who want to use errors in event streams need to use them as so:
```rust,ignore
stream! {
    yield Ok(EventStreamUnion::ErrorVariant ...)
}
```
Furthermore, there is no support for `@error`s in event streams being terminal; that is, when an error is sent,
it does not signal termination and thus does not complete the stream.

This RFC proposes to make changes to:
* terminate the stream upon receiving a modeled error
* change the API so that customers will write their business logic in a more Rust-like experience:
```rust,ignore
stream! {
    yield Err(EventStreamUnionError::ErrorKind ...)
}
```
Thus any `Err(_)` from the stream is terminal, rather than any `Ok(x)` with `x` being matched against the set of modeled variant errors in the union.

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

In order to implement this feature:
* Errors modeled in streaming unions are going to be treated like operation errors
  * They are in the `error::` namespace
  * They have the same methods operation errors have (`name` on the server, `metadata` on the client and so on)
  * They are not variants in the corresponding error structure
* Errors need to be marshalled and unmarshalled
* `Receiver` must treat any error coming from the other end as terminal

The code examples below have been generated using the [following model](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen-server-test/model/pokemon.smithy#L27):
```smithy
@http(uri: "/capture-pokemon-event/{region}", method: "POST")
operation CapturePokemonOperation {
    input: CapturePokemonOperationEventsInput,
    output: CapturePokemonOperationEventsOutput,
    errors: [UnsupportedRegionError, ThrottlingError]
}

@input
structure CapturePokemonOperationEventsInput {
    @httpPayload
    events: AttemptCapturingPokemonEvent,

    @httpLabel
    @required
    region: String,
}

@output
structure CapturePokemonOperationEventsOutput {
    @httpPayload
    events: CapturePokemonEvents,
}

@streaming
union AttemptCapturingPokemonEvent {
    event: CapturingEvent,
    masterball_unsuccessful: MasterBallUnsuccessful,
}

structure CapturingEvent {
    @eventPayload
    payload: CapturingPayload,
}

structure CapturingPayload {
    name: String,
    pokeball: String,
}

@streaming
union CapturePokemonEvents {
    event: CaptureEvent,
    invalid_pokeball: InvalidPokeballError,
    throttlingError: ThrottlingError,
}

structure CaptureEvent {
    @eventHeader
    name: String,
    @eventHeader
    captured: Boolean,
    @eventHeader
    shiny: Boolean,
    @eventPayload
    pokedex_update: Blob,
}

@error("server")
structure UnsupportedRegionError {
    @required
    region: String,
}
@error("client")
structure InvalidPokeballError {
    @required
    pokeball: String,
}
@error("server")
structure MasterBallUnsuccessful {
    @required
    message: String,
}
@error("client")
structure ThrottlingError {}
```
Wherever irrelevant, documentation and other lines are stripped out from the code examples below.

#### Errors in streaming unions

The error in `AttemptCapturingPokemonEvent` is modeled as follows.

On the client,
```rust,ignore
pub struct AttemptCapturingPokemonEventError {
    pub kind: AttemptCapturingPokemonEventErrorKind,
    pub(crate) meta: aws_smithy_types::Error,
}
pub enum AttemptCapturingPokemonEventErrorKind {
    MasterBallUnsuccessful(crate::error::MasterBallUnsuccessful),
    Unhandled(Box<dyn std::error::Error + Send + Sync + 'static>),
}
```

On the server,
```rust,ignore
pub enum AttemptCapturingPokemonEventError {
    MasterBallUnsuccessful(crate::error::MasterBallUnsuccessful),
}
```

Both are modeled as normal errors, where the [name](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/generators/error/CombinedErrorGenerator.kt#L50) comes from `Error` with a prefix of the union's name.
In fact, both the [client](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/generators/error/CombinedErrorGenerator.kt#L71) and [server](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerCombinedErrorGenerator.kt#L46)
generate operation errors and event stream errors the same way.

Event stream errors have their own [marshaller](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/protocols/serialize/EventStreamErrorMarshallerGenerator.kt#L39).
To make it work for users to stream errors, `EventStreamSender<>`, in addition to the union type `T`, takes an error type `E`; that is, the `AttemptCapturingPokemonEventError` in the example.
This means that an error from the stream is [marshalled and sent](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/sender.rs#L137) as a data structure similarly to the union's non-error members.

On the other side, the `Receiver<>` needs to terminate the stream upon [receiving any error](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/receiver.rs#L249).
A terminated stream has [no more data](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/receiver.rs#L38) and will always be a [bug](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/receiver.rs#L54) to use it.

An example of how errors can be used on clients, extracted from [this test](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http-server/examples/pokemon_service/tests/simple_integration_test.rs#L100):
```rust,ignore
yield Err(AttemptCapturingPokemonEventError::new(
    AttemptCapturingPokemonEventErrorKind::MasterBallUnsuccessful(MasterBallUnsuccessful::builder().build()),
    Default::default()
));
```

Because unions can be used in input or output of more than one operation, errors must be generated once as they are in the `error::` namespace.

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [x] Errors are in the `error::` namespace and created as operation errors
- [x] Errors can be sent to the stream
- [x] Errors terminate the stream
- [x] Customers' experience using errors mirrors the Rust way: `Err(error::StreamingError ...)`
