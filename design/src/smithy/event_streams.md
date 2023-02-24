Event Streams
=============

Event streams are described in [the smithy specification](https://awslabs.github.io/smithy/1.0/spec/core/stream-traits.html#event-streams).

This document describes:
* What protocols are supported
* How the `@streaming union`s are generated
* How errors are implemented
* How the output code looks like
* How users will implement their business logic with the new types
* How errors and data are sent between client and server

The user experience
----------------------------------------------

Let us take [the following model](https://github.com/awslabs/smithy-rs/pull/1479/files#diff-ae332acd4a848e840d018d3b8616e40031f9e8f96ed89777dea69eb1f51a89a4R25) as an example:
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

Clients will write their business logic, for example, as:
1. Create the stream and...
```rust
let input_stream = stream! {
```
2. ...send some union variant
```rust
yield Ok(AttemptCapturingPokemonEvent::Event(
    CapturingEvent::builder()
    .payload(CapturingPayload::builder()
        .name("Pikachu")
        .pokeball("Master Ball")
        .build())
    .build()
));
```
3. ...terminate the stream by exhausting the stream or by sending an error variant
```rust
yield Err(AttemptCapturingPokemonEventError::new(
    AttemptCapturingPokemonEventErrorKind::MasterBallUnsuccessful(MasterBallUnsuccessful::builder().build()),
    Default::default()
));
```
4. Open the stream and start communicating with the server
```rust
let mut output = client
    .capture_pokemon_operation()
    .region("Kanto")
    .events(input_stream.into())
    .send()
    .await
    .unwrap();
loop {
    match output.events.recv().await {
        Ok(Some(capture)) => {...}
        Err(e) => {... break;} /* terminated with an error */
        Ok(None) => break, /* terminated cleanly */
    }
}
```

Similarly, on the server side, the business logic might look like:
```rust
pub async fn capture_pokemon(
    mut input: input::CapturePokemonOperationInput,
) -> Result<output::CapturePokemonOperationOutput, error::CapturePokemonOperationError> {
    if input.region != "Kanto" {
        /* operation error */
        return Err(error::CapturePokemonOperationError::UnsupportedRegionError(
            error::UnsupportedRegionError::builder().build(),
        ));
    }
    /* create the stream, but... */
    let output_stream = stream! {
        loop {
            match input.events.recv().await {
                Ok(maybe_event) => match maybe_event {
                    Some(event) => {...} /* handle event */
                    None => break, /* stream ended cleanly */
                },
                Err(e) => {...} /* stream ended by error */
            }
        }
    };
    /* ...return immediately, and communicate async over the stream, not the current connection */
    Ok(output::CapturePokemonOperationOutput::builder()
        .events(output_stream.into())
        .build()
        .unwrap())
}
```

Implementation
----------------------------------

In order to implement this feature for all customers, we need to:
* Implement a way to transparently stream data, and handle signing and terminate the stream on errors or the termination condition
* Implement custom marshalling and unmarshalling for errors and streaming union variants
* Make it easy and obvious to customers to use the generated types, structures and functions

The Input and Output structures are generated as (lines irrelevant for the discussion are omitted, here and in other examples):

```rust
// on the client
pub struct CapturePokemonOperationInput {
    #[allow(missing_docs)] // documentation missing in model
    pub events: aws_smithy_http::event_stream::EventStreamSender<
        crate::model::AttemptCapturingPokemonEvent,
        crate::error::AttemptCapturingPokemonEventError,
    >,
    #[allow(missing_docs)] // documentation missing in model
    pub region: std::option::Option<std::string::String>,
}
// on the server
pub struct CapturePokemonOperationInput {
    #[allow(missing_docs)] // documentation missing in model
    pub events: aws_smithy_http::event_stream::Receiver<
        crate::model::AttemptCapturingPokemonEvent,
        crate::error::AttemptCapturingPokemonEventError,
    >,
    #[allow(missing_docs)] // documentation missing in model
    pub region: std::string::String,
}
```
Note they are similar, but the client uses an `EventStreamSender` (this is an input structure) and the server a `Receiver` (the server receives the input).
Sender is [chosen according](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/EventStreamSymbolProvider.kt#L58) to if the
generation target is the client and the union is in an input structure; or if this is a server sending from an operation's output.

The error structs are generated as any operation error, where the union shape is treated as an operation:
```rust
// on the client
pub struct AttemptCapturingPokemonEventError {
   pub kind: AttemptCapturingPokemonEventErrorKind,
   pub(crate) meta: aws_smithy_types::Error,
}
pub enum AttemptCapturingPokemonEventErrorKind {
   MasterBallUnsuccessful(crate::error::MasterBallUnsuccessful),
   Unhandled(Box<dyn std::error::Error + Send + Sync + 'static>),
}
// on the server
pub enum AttemptCapturingPokemonEventError {
    MasterBallUnsuccessful(crate::error::MasterBallUnsuccessful),
}
```
The errors are similar to any operation error, but their [name](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/generators/error/CombinedErrorGenerator.kt#L50) is `Error` prefixed by the name of the union.
The reason for splitting up errors and non-errors in the event stream union is to give a more Rust-like experience to customers,
where they can `yield` and `match` on errors `Err(UnionError::Variant)` as a result of the event stream communication,
rather than matching on the specific variant, `Ok(Union::Variant) => /* handle error */`.

If there are no modeled errors in the union, because the server does not always generate an error structure unlike the client (which at least has a `Unhandled` variant),
the server uses a generic `MessageStreamError` in place of the union error, to be signaled of any error while constructing the stream.
If the union were to be:
```smithy
@streaming
union AttemptCapturingPokemonEvent {
    event: CapturingEvent,
}
```
The Receiver would be
```rust
aws_smithy_http::event_stream::Receiver<
  crate::model::AttemptCapturingPokemonEvent,
  aws_smithy_http::event_stream::MessageStreamError>
```
and errors propagated as such to terminate the stream.

An [EventStreamSender](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/sender.rs#L18) wraps an input stream
into a [MessageStreamAdapter](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/sender.rs#L132), which implements
the [Stream](https://docs.rs/futures-core/0.3.21/futures_core/stream/trait.Stream.html) trait. At a high level, `poll_next` works by:
1. Polling the customer stream
2. If there is an event:
   1. Signal the end of the stream, if the event is `None`
   2. Marshall either the message or the error, depending on the data coming from the stream
   3. Sign the marshalled data and return it
3. Otherwise, signal to poll back later

Similarly, the [Receiver](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-http/src/event_stream/receiver.rs#L125) handles the receiving side of the stream.
The `Receiver` has more logic to handle buffering data and possibly errors.

Server and client serialize similarly. Serializing for `CapturePokemonOperation` on the server, with `serialize_capture_pokemon_operation_response`:
1. Sets the `content-type` HTTP header to `application/vnd.amazon.eventstream`
2. Converts the `EventStreamSender` in the event stream structure into a `MessageStreamAdapter` with a marshaller for the error and data types
   1. [This](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/ServerHttpBoundProtocolGenerator.kt#L511) is where it is generated
3. Gives the body back to hyper
```rust
let body =
   aws_smithy_http_server::body::boxed(aws_smithy_http_server::body::Body::wrap_stream({
      let signer = aws_smithy_eventstream::frame::NoOpSigner {};
      let error_marshaller =
           crate::event_stream_serde::CapturePokemonOperationErrorMarshaller::new();
       let marshaller = crate::event_stream_serde::CapturePokemonEventsMarshaller::new();
       let adapter: aws_smithy_http::event_stream::MessageStreamAdapter<_, _> = output
           .events
           .into_body_stream(marshaller, error_marshaller, signer);
       adapter
   }));
```
The signer signs the message to be sent. [NoOpSigner](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-eventstream/src/frame.rs#L37) returns the message as is, `SigV4Signer` signs using the AWS SigV4 protocol. SigV4 requires an empty-payload signed message to be sent before effectively terminating the stream; to keep the same interface, `SignMessage::sign_empty` returns an `Option` to signal whether signing this last empty message is required.

The marshallers set header and payload in a [Message](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-eventstream/src/frame.rs#L368) structure: `Message` is a structure with a vector, the headers; and bytes, the payload.
The headers are the values targeted by the `@eventHeader` trait and the payload by `@eventPayload`.

At the end of the marshalling and signing processes, `MessageStreamAdapter` takes the `Message` built by the marshaller and signer
and [writes](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/rust-runtime/aws-smithy-eventstream/src/frame.rs#L224) it as bytes into a `Vec<u8>`,
in a format of a sequence of bytes: `<type, data>` where `type` indicates if the `data` is a bool, integer and so on for all types.

Headers that are sent are:
* `:message-type` (`event` or `exception`) to signal the kind of message being sent
* `:content-type`, set to `application/octet-stream` for blobs; the protocol-specific type otherwise
* `:event-type` to communicate the non-error variant (if `:message-type` is `event`); or
* `:exception-type` to communicate the error variant (if `:message-type` is `exception`)

The way errors are marshalled, unmarshalled and signed is the same as above.

#### Generating errors
Event stream errors in unions are generated in the same way for operation errors:
In fact, the implementation uses the same [render](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerCombinedErrorGenerator.kt#L47) functions;
the only difference between client and server is that the server does not generate anything unless the structure has errors,
while the client always generates a structure for forward compatibility with at least a `Unhandled` error kind.
This is also the reason for the default [MessageStreamError](https://github.com/awslabs/smithy-rs/blob/8f7e03ff8a84236955a65dba3d21c4bdbf17a9f4/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/EventStreamSymbolProvider.kt#L52) for servers.

The main differences between the EventStreamErrorMarshallerGenerator and EventStreamMarshallerGenerator are that the former:
* takes into account the differences between client and server in how error symbols are laid out (with a `kind` member or `Kind` suffix)
* sets the `":message-type"` to be `exception` and sets `:exception-type` accordingly

Currently, the only supported protocols are:
* RestXml
* RestJSON
