---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: true
new_feature: true
bug_fix: false
---
`aws-smithy-http-server` now provides `serve::bind` for binding and serving TCP listeners with runtime defaults. Generated HTTP 1.x server documentation now uses and re-exports `bind` alongside `serve`.

Servers built with `aws-smithy-http-server::serve::serve` or `bind` now limit concurrently accepted connections to 8192 by default. Use `.max_connections(...)` to set a different limit, or `.disable_connection_limit()` to restore the previous unbounded behavior.

Listeners created by `bind` use a default socket listen backlog of 1024 and enable `TCP_NODELAY` and `SO_KEEPALIVE` by default. Use `.socket_listen_backlog(...)`, `.tcp_nodelay(...)`, and `.tcp_keepalive(...)` to override these TCP settings.
