---
applies_to:
- server
authors:
- rcoh
- jasgin
references: []
breaking: false
new_feature: false
bug_fix: true
---
Make `DefaultMetricsPlugin`'s `Service` impl generic over the request body type instead of
hardcoding `hyper::body::Incoming`. The plugin only reads request *extensions* (operation name,
service name, request ID) and never touches the body, so it composes with any body type. This
fixes a compilation error when using `DefaultMetricsPlugin` with request bodies other than
`hyper::body::Incoming`.
