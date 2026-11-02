---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: false
bug_fix: true
---
Python servers now limit accepted connections to 16,384 per worker for both HTTP and TLS, including connections performing TLS handshakes. Excess connections wait in the socket backlog until a slot is released. HTTP/1 request headers have a 60-second read deadline. Regenerated Python SDKs expose `app.run(max_connections=...)` to customize the positive per-worker limit; existing SDKs receive the default protection when rebuilt with the updated runtime.

These defaults change previously unbounded behavior: connections above the cap wait for capacity, and connections whose HTTP/1 headers take longer than 60 seconds are closed. Services relying on either behavior must account for the new limits when upgrading to `aws-smithy-http-server-python` 0.71.0.
