---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: true
new_feature: true
bug_fix: true
---
`aws-smithy-http-server::serve` now bounds initial HTTP version detection to 30 seconds, closing clients that send no bytes or an incomplete HTTP/2 preface. Configure this deadline with `.http_version_detection_timeout(Some(duration))`, or pass `None` to disable it. It remains enabled with a custom Hyper builder and ends once HTTP/1 or HTTP/2 is identified. Connection permits now stay attached to the IO across HTTP upgrades, so upgraded sockets continue to count toward `.max_connections(...)`.

Applications that downcast upgraded connections must target `TokioIo<ConnLimiterIo<OriginalIo>>` with `hyper_util::server::conn::auto::upgrade::downcast`. Service factories still receive the original listener IO through `IncomingStream`.
