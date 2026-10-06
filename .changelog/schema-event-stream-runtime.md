---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Schema-mode event stream marshalling and unmarshalling moved from generated code into
`aws_smithy_http_server::schema::event_stream`. Frames are now built and read against the
shapes' runtime schemas — the member schemas carry `@eventHeader` and `@eventPayload` — so the
generated `crate::event_stream_serde` module shrinks to type aliases
(`<Union>Marshaller`, `<Union>Unmarshaller`, `<Union>ErrorMarshaller` keep their names and
constructors) plus one dispatch per stream union naming its events and errors.

Wire behavior is preserved, with two deliberate improvements:

- An optional structured `@eventPayload` member that is `None` now marshals as an empty payload
  (with the usual `:content-type`) instead of panicking with `unimplemented!()`.
- An event struct whose members carry `@eventHeader` traits is now unmarshalled by routing those
  headers even when it is a modeled error; previously error frames were always read whole from
  the payload.

`serialize_streaming_output` also no longer interpolates the
`alwaysSendEventStreamInitialResponse` codegen setting as a boolean literal inside a condition
(which tripped `clippy::overly_complex_bool_expr`); the setting is passed as a plain argument to
the runtime glue.

AWS JSON multi-protocol routing recognizes the event-stream media type for modeled
streaming union inputs without collecting their bodies. Requests without a JSON
version marker follow the existing protocol priority when both versions are enabled.
Modeled exception frames also retain AWS JSON's `__type` discriminator (full shape
ID in 1.0, shape name in 1.1).
