---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

The schema-driven router (`SchemaRoutingService`) now shares one handler set across all clones of
the service instead of copying every route per request. To make that possible, schema-path routes
are stored as the new `aws_smithy_http_server::routing::SyncRoute`, which, unlike `Route`, is
`Sync`:

- Generated schema-path service builders require handlers, HTTP plugin outputs and layers
  (`build`, `build_unchecked`, `layer`, `*_custom`) to be `Send + Sync`.
- `OperationHandlerBinding::new` and `SchemaRoutingService::layer` take and produce `SyncRoute`.
- The operation's request-body limits now travel with the routed request:
  `SelectedProtocolOperation::new` takes the operation's `RequestBodyCollectionConfig` as a third
  argument (read it back with `request_body_config()`), and `DynUpgradePlugin::new()` /
  `StreamingUpgradePlugin::new()` no longer take a config argument. An HTTP plugin that re-inserts
  `SelectedProtocolOperation` should carry the existing `request_body_config()` over.
- `aws-smithy-http-server` now depends on `tower` 0.5 (for `BoxCloneSyncService`). The `Service`
  and `Layer` traits are the same `tower-service` / `tower-layer` traits as before.

Generated servers that do not use the schema path are unchanged: `Route` and the protocol routers
keep their previous bounds, so handlers, plugins and layers that are not `Sync` still compile.
