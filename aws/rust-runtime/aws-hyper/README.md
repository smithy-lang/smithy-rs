# AWS Default Middleware

This crate defines the default middleware stack used by AWS services. It also provides a re-export
of `aws_smithy_client::Client` with the middleware type preset.

_Note:_ this crate will be removed in the future in favor of defining the middlewares directly in the service clients.
