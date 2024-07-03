# Detailed Error Explanations

This page collects detailed explanations for some errors. If you encounter an
error and are interested in learning more about what it means and why it occurs,
check here.

**If you can't find the explanation on this page, please file an issue asking
for it to be added.**

## "Connection encountered an issue and should not be re-used. Marking it for closure"

The SDK clients each maintain their own connection pool (_except when they share
an `HttpClient`_). By the convention of some services, when a request fails due
to a [transient error](#transient-errors), that connection should not be re-used
for a retry. Instead, it should be dropped and a new connection created instead.
This prevents clients from repeatedly sending requests over a failed connection.

This feature is referred to as "connection poisoning" internally.

## Transient Errors

When requests to a service time out, or when a service responds with a 500, 502,
503, or 504 error, it's considered a 'transient error'. Transient errors are
often resolved by making another request.

When retrying transient errors, the SDKs may avoid re-using connections to
overloaded or otherwise unavailable service endpoints, choosing instead to
establish a new connection. This behavior is referred to internally as
"connection poisoning" and is configurable.

To configure this behavior, set the [reconnect_mode][reconnect-mode] in an SDK
client config's [RetryConfig].

[file an issue]: https://github.com/smithy-lang/smithy-rs/issues/new?assignees=&labels=&projects=&template=blank_issue.md
[RetryConfig]: https://docs.rs/aws-types/latest/aws_types/sdk_config/struct.RetryConfig.html
[reconnect-mode]: https://docs.rs/aws-types/latest/aws_types/sdk_config/struct.RetryConfig.html#method.with_reconnect_mode
