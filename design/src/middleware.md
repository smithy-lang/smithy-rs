# HTTP middleware

Signing, endpoint specification, and logging are all handled as middleware. The Rust SDK takes a minimalist approach to middleware:

Middleware is defined as minimally as possible, then adapted into the middleware system used by the IO layer. Tower is the de facto standard for HTTP middleware in Rust—we will probably use it. But we also want to make our middleware usable for users who aren't using Tower (or if we decide to not use Tower in the long run).

Because of this, rather than implementing all our middleware as "Tower Middleware", we implement it narrowly (eg. as a function that operates on `operation::Request`), then define optional adapters to make our middleware tower compatible.
