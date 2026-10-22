# Event stream request reads

Generated servers cooperatively yield when an event stream request body stays ready.
By default, after 32 consecutive ready HTTP body frames, the next poll returns
`Pending` and calls the current task's waker. It does not consume another frame
during that yield. This gives the executor an opportunity to run other tasks.
No background task or thread is created.

Teams configure the budget in the `rust-server-codegen` plugin's settings in
`smithy-build.json`:

```json
{
  "customizationConfig": {
    "protocols": {
      "common": {
        "eventStreamReadPollBudget": 64
      }
    }
  }
}
```

Omitting the setting uses `32`. Setting it to `0` disables forced yielding.
Values must be integers from `0` through `2147483647`. The setting applies to
all event stream request inputs generated for the service, including RPC initial
frames. Regenerate and rebuild the service to change it; it is not a live runtime
setting.

The budget counts HTTP body frames, not decoded events or bytes. Empty data
frames, trailers, and errors count. A natural `Pending` resets the counter.
A smaller budget gives more frequent scheduling opportunities; a larger budget
reduces forced yields. An individual frame can still be large, and this mechanism
does not preempt an individual body poll or application handler.

The budget applies even when message timeouts are disabled. Teams can configure
both independently:

```json
{
  "customizationConfig": {
    "protocols": {
      "common": {
        "eventStreamReadPollBudget": 32,
        "requestBodyReadTimeouts": {
          "defaultNonPayload": "5m",
          "defaultPayload": "1h"
        },
        "eventStreamTimeouts": {
          "initialMessage": "10s",
          "messageCompletion": "30s",
          "idle": "1h",
          "perOperation": {
            "com.example#PublishEvents": {
              "initialMessage": "5s",
              "messageCompletion": "10s"
            }
          }
        }
      }
    }
  }
}
```

The common protocol section also contains the request body read timeout
configuration for non-streaming operations. `defaultNonPayload` defaults to `5m`;
`defaultPayload` defaults to `1h`. A `perOperation` entry overrides the applicable
default for non-streaming operations. Setting a timeout to `0` disables it.

Event stream timeouts have their own object,
`customizationConfig.protocols.common.eventStreamTimeouts`, with three independent
values:

- `initialMessage` applies to the existing RPC first-frame read before the handler runs,
  including operations whose input contains only an event stream.
  Its deadline starts when that read is set up, before waiting for the first byte,
  and runs until the entire initial frame arrives. Receiving its first bytes does
  not reset the deadline. If the value is `10s` and the first bytes arrive after
  `8s`, only `2s` remain to complete the frame.
- `messageCompletion` applies to each subsequent event message. Each deadline
  starts when that message's first bytes are read and ends when the complete frame
  arrives.
- `idle` bounds waiting between a complete frame and the next event's first bytes.
  It defaults to `0` (disabled). When configured, REST streams also use it while waiting for their first event;
  RPC streams start it after the pre-handler first frame completes.
  First bytes end the idle
  deadline and start `messageCompletion`, when configured. Empty body chunks do not
  reset the idle deadline.

`initialMessage`, `messageCompletion`, and `idle` default to `0` (disabled). All three values
accept durations with an explicit `ms`, `s`, `m`, or `h` unit, or `0` to disable,
and can be overridden independently under `perOperation`.
An operation override of `0` disables just that deadline; an omitted value uses
the corresponding common default. Operation IDs must belong to this service and
have an event stream input.

REST event streams have no pre-handler initial-message read. `initialMessage`
does not apply to them; `messageCompletion` covers their first event and all later
events. RPC streams preserve the existing first-frame read before entering the handler,
even when their inputs contain no non-stream fields. That frame uses only
`initialMessage`, even when its deadline is disabled. The event deadline never
shortens the initial deadline.
The RPC initial read examines the first wire frame. If an optional `initial-request`
message is omitted, the first event is read during that phase and uses the initial
deadline; its already-complete frame is then buffered for the handler.

All event stream deadlines are configured exclusively through
`customizationConfig.protocols.common.eventStreamTimeouts`; an empty object
disables all three event stream deadlines, and omitting the object leaves them
at their defaults (all disabled). The poll budget is configured exclusively
through `customizationConfig.protocols.common.eventStreamReadPollBudget`.
Use `requestBodyReadTimeouts` for non-streaming request bodies only; entries
for streaming operations, including event stream inputs, fail generation.

When configured, the message deadline
is checked before the polling budget can yield. Yielding does not reset or extend
that deadline. Time between polls counts toward the deadline.

An event-completion or idle timeout makes `recv()` return an error whose
source chain contains the generated crate's public
`event_stream_message_timeout::MessageTimeoutError`. Handlers can detect it with
`MessageTimeoutError::find_in_chain`, then call `recv()` again for another timeout
window or stop processing the request. `MessageTimeoutError::is_idle()` distinguishes
idle expiry from message-completion expiry. The timeout itself does not close the
transport connection. RPC initial-message deadlines include the first-byte wait
and become `408 Request Timeout` rejections before the handler runs. Waiting for the first byte
of a later event message uses the idle deadline. Time spent between body polls,
including handler backpressure, counts toward the current deadline.
