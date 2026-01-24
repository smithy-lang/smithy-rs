# Smithy-RS Server Request Flow Trace

This document traces the complete path of an HTTP request through a smithy-rs generated server, from when Hyper accepts the connection to when the response is returned.

## Middleware Positions

The smithy-rs server has **4 middleware positions** (A, B, C, D) where custom logic can be inserted:

| Position | Name | When it Runs | Configured Via |
|----------|------|--------------|----------------|
| **A** | Outer Middleware | ALL requests (even routing failures) | `Layer::layer(app)` after building |
| **B** | Route Middleware | After routing succeeds | `PokemonServiceConfig::builder().layer()` |
| **C** | HTTP Plugins | Per-operation, on HTTP Request/Response | `.http_plugin(HttpPlugins::new())` |
| **D** | Model Plugins | Per-operation, on Model Input/Output | `.model_plugin(ModelPlugins::new())` |

## Complete Trace Output (with all middleware)

When running the `simple-client` against the `pokemon-service`, here is the complete trace for a `GET /stats` request to the `GetServerStatistics` operation:

```
[TRACE 1] ========== NEW CONNECTION ==========
[TRACE 1] File: aws-smithy-http-server/src/serve/mod.rs
[TRACE 1] Function: handle_connection()
[TRACE 1] Connection accepted from: 127.0.0.1:56288
[TRACE 1] ========================================

[TRACE 2] File: aws-smithy-http-server/src/serve/mod.rs
[TRACE 2] Calling make_service.ready() to prepare service factory
[TRACE 3] File: aws-smithy-http-server/src/serve/mod.rs
[TRACE 3] Calling make_service.call() to create per-connection service
[TRACE 5] File: aws-smithy-http-server/src/routing/into_make_service.rs
[TRACE 5] Type: IntoMakeService<S>
[TRACE 5] Function: Service::call() - Cloning router service for new connection
[TRACE 4] File: aws-smithy-http-server/src/serve/mod.rs
[TRACE 4] Wrapping Tower service with TowerToHyperService for Hyper compatibility

[TRACE A1] ========== OUTER MIDDLEWARE (Position A) ==========
[TRACE A1] File: pokemon-service/src/outer_middleware.rs
[TRACE A1] Type: OuterMiddlewareService<S>
[TRACE A1] This is the OUTERMOST layer - ALL requests pass through
[TRACE A1] Method: GET | URI: /stats
[TRACE A1] ===========================================================

[TRACE 6] ========== HTTP REQUEST RECEIVED ==========
[TRACE 6] File: aws-smithy-http-server/src/routing/mod.rs
[TRACE 6] Type: RoutingService<R, Protocol>
[TRACE 6] Function: Service::call()
[TRACE 6] Method: GET | URI: /stats
[TRACE 6] ==============================================

[TRACE 7] File: aws-smithy-http-server/src/routing/mod.rs
[TRACE 7] Calling Router::match_route() to find matching operation...
[TRACE 9] File: aws-smithy-http-server/src/protocol/rest/router.rs
[TRACE 9] Type: RestRouter<S>
[TRACE 9] Function: Router::match_route()
[TRACE 9] Searching 7 routes for match...
[TRACE 10] Route[0]: Checking RequestSpec - Result: No
[TRACE 10] Route[1]: Checking RequestSpec - Result: No
[TRACE 10] Route[2]: Checking RequestSpec - Result: No
[TRACE 10] Route[3]: Checking RequestSpec - Result: No
[TRACE 10] Route[4]: Checking RequestSpec - Result: No
[TRACE 10] Route[5]: Checking RequestSpec - Result: Yes
[TRACE 11] File: aws-smithy-http-server/src/protocol/rest/router.rs
[TRACE 11] MATCH FOUND at route index 5!
[TRACE 8] File: aws-smithy-http-server/src/routing/mod.rs
[TRACE 8] Route matched! Dispatching to operation service via oneshot()

[TRACE B2] File: aws-smithy-http-server/src/layer/alb_health_check.rs
[TRACE B2] Type: AlbHealthCheckService<H, S> (Route Middleware - Position B)
[TRACE B2] Checking if URI matches health check path: /ping
[TRACE B2] Not a health check, passing through to inner service
[TRACE B1] File: aws-smithy-http-server/src/request/request_id.rs
[TRACE B1] Type: ServerRequestIdProvider<S> (Route Middleware - Position B)
[TRACE B1] Generated ServerRequestId: ad4f5721-6d97-4f3f-811a-2c821abf2ab9
[TRACE B1] Adding request ID to request extensions

[TRACE 12] File: aws-smithy-http-server/src/routing/route.rs
[TRACE 12] Type: Route<B> (type-erased service wrapper)
[TRACE 12] Function: Service::call()
[TRACE 12] Forwarding request to BoxCloneService (the actual operation service)

[TRACE 13] ========== UPGRADE SERVICE ==========
[TRACE 13] File: aws-smithy-http-server/src/operation/upgrade.rs
[TRACE 13] Type: Upgrade<Protocol, Input, S>
[TRACE 13] Function: Service::call()
[TRACE 13] Starting HTTP -> Smithy type conversion
[TRACE 13] Phase: FromRequest - deserializing HTTP request to operation Input
[TRACE 13] ==========================================

[TRACE 14b] File: aws-smithy-http-server/src/request/extension.rs
[TRACE 14b] Type: Extension<T>
[TRACE 14b] Function: FromParts::from_parts()
[TRACE 14b] Extracting Extension<T> from request parts
[TRACE 14b] Extension extracted successfully!
[TRACE 14] File: aws-smithy-http-server/src/operation/upgrade.rs
[TRACE 14] Type: UpgradeFuture (FromRequest phase complete)
[TRACE 14] Request deserialization SUCCESSFUL!
[TRACE 14] Now calling inner operation service with deserialized input...

[TRACE D1] ========== MODEL PLUGIN: AuthorizationPlugin ==========
[TRACE D1] File: pokemon-service/src/authz.rs
[TRACE D1] Type: AuthorizeService<Op, S> (Model Plugin - Position D)
[TRACE D1] This runs AFTER Upgrade (on Model Input/Output)
[TRACE D1] Checking authorization before calling handler...
[TRACE D1] ===========================================================

[TRACE D2] Authorization PASSED! Calling inner handler...

[TRACE 15] ========== HANDLER EXECUTION ==========
[TRACE 15] File: aws-smithy-http-server/src/operation/handler.rs
[TRACE 15] Type: IntoService<Op, H>
[TRACE 15] Function: Service::call()
[TRACE 15] Calling the user's handler function with deserialized input + extensions
[TRACE 15] ==============================================

[TRACE 17] File: aws-smithy-http-server/src/operation/upgrade.rs
[TRACE 17] Type: UpgradeFuture (Inner handler phase complete)
[TRACE 17] Handler returned SUCCESS - converting output to HTTP response
[TRACE 18] File: aws-smithy-http-server/src/operation/upgrade.rs
[TRACE 18] Response serialization complete, returning HTTP response

[TRACE A2] ========== OUTER MIDDLEWARE RESPONSE ==========
[TRACE A2] File: pokemon-service/src/outer_middleware.rs
[TRACE A2] Response Status: 200 OK
[TRACE A2] Request completed successfully through outer middleware
[TRACE A2] ======================================================
```

**Note:** The `PrintPlugin` (Position C - HTTP Plugin) is not shown because it's scoped only to
`GetPokemonSpecies` and `GetStorage` operations, not `GetServerStatistics`.

## ASCII Flow Diagram (with Middleware Positions A, B, C, D)

```
                    SMITHY-RS SERVER REQUEST FLOW WITH MIDDLEWARE
                    =============================================

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │                              TCP CONNECTION PHASE                                │
 └─────────────────────────────────────────────────────────────────────────────────┘

    Client                                                              Server
      │                                                                    │
      │  TCP Connect                                                       │
      │ ─────────────────────────────────────────────────────────────────► │
      │                                                                    │
      │                    ┌─────────────────────────────────┐             │
      │                    │ serve/mod.rs                    │             │
      │                    │ handle_connection()             │             │
      │                    │                                 │             │
      │                    │ [TRACE 1] Connection accepted   │             │
      │                    │ from: 127.0.0.1:XXXXX          │             │
      │                    └──────────────┬──────────────────┘             │
      │                                   │                                │
      │                                   ▼                                │
      │                    ┌─────────────────────────────────┐             │
      │                    │ [TRACE 2-5] make_service.call() │             │
      │                    │ IntoMakeService clones router   │             │
      │                    │ TowerToHyperService wraps it    │             │
      │                    └──────────────┬──────────────────┘             │
      │                                   │                                │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  POSITION A: OUTER MIDDLEWARE                                                    │
 │  ════════════════════════════                                                    │
 │  ALL requests pass through here, even routing failures                          │
 └─────────────────────────────────────────────────────────────────────────────────┘

      │  GET /stats HTTP/1.1                                               │
      │ ─────────────────────────────────────────────────────────────────► │
      │                                                                    │
      │                    ┌─────────────────────────────────┐             │
      │                    │ [TRACE A1] OUTER MIDDLEWARE     │             │
      │                    │ outer_middleware.rs             │             │
      │                    │ OuterMiddlewareService::call()  │             │
      │                    │                                 │             │
      │                    │ Method: GET | URI: /stats       │             │
      │                    └──────────────┬──────────────────┘             │
      │                                   │                                │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │                              ROUTING PHASE                                       │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE 6] routing/mod.rs        │
                           │ RoutingService::call()          │
                           │ HTTP REQUEST RECEIVED           │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ [TRACE 9-11] RestRouter         │
                           │ Searching 7 routes...           │
                           │                                 │
                           │  Route[5]: /stats → Match!      │
                           │  (GetServerStatistics)          │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  POSITION B: ROUTE MIDDLEWARE                                                    │
 │  ════════════════════════════                                                    │
 │  Runs AFTER routing succeeds, applied to all matched routes                     │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE B2] AlbHealthCheckService│
                           │ alb_health_check.rs             │
                           │                                 │
                           │ Checking: /stats != /ping       │
                           │ → Pass through to inner         │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ [TRACE B1] ServerRequestId      │
                           │ request_id.rs                   │
                           │                                 │
                           │ Generated: ad4f5721-6d97-...    │
                           │ → Added to request extensions   │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ AddExtensionLayer (tower_http)  │
                           │                                 │
                           │ Adds Arc<State> to extensions   │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ [TRACE 12] Route::call()        │
                           │ Forwards to operation pipeline  │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  POSITION C: HTTP PLUGINS                                                        │
 │  ════════════════════════                                                        │
 │  Operation-specific, runs on HTTP Request/Response (BEFORE deserialization)     │
 │  Note: PrintPlugin only scoped to GetPokemonSpecies/GetStorage, not shown here  │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE C1] PrintPlugin          │
                           │ (if operation in scope)         │
                           │ plugin.rs                       │
                           │                                 │
                           │ Logs operation + service name   │
                           │ (NOT called for /stats)         │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │                         DESERIALIZATION PHASE (Upgrade)                          │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE 13] Upgrade::call()      │
                           │ operation/upgrade.rs            │
                           │                                 │
                           │ HTTP → Smithy type conversion   │
                           └──────────────┬──────────────────┘
                                          │
                          ┌───────────────┴───────────────┐
                          │                               │
                          ▼                               ▼
           ┌──────────────────────────┐    ┌──────────────────────────┐
           │ FromRequest (generated)  │    │ [TRACE 14b] FromParts    │
           │                          │    │ extension.rs             │
           │ Deserialize HTTP body    │    │                          │
           │ → GetServerStatisticsIn  │    │ Extract Extension<State> │
           └──────────────┬───────────┘    └──────────────┬───────────┘
                          │                               │
                          └───────────────┬───────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ [TRACE 14] Deserialization OK   │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  POSITION D: MODEL PLUGINS                                                       │
 │  ═════════════════════════                                                       │
 │  Operation-specific, runs on Model Input/Output (AFTER deserialization)         │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE D1] AuthorizationPlugin  │
                           │ authz.rs                        │
                           │ AuthorizeService::call()        │
                           │                                 │
                           │ Checking authorization...       │
                           │ [TRACE D2] Authorization PASSED │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │                         HANDLER EXECUTION PHASE                                  │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE 15] IntoService::call()  │
                           │ operation/handler.rs            │
                           │                                 │
                           │ Calling user's handler function │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ get_server_statistics()         │
                           │ pokemon-service-common/lib.rs   │
                           │                                 │
                           │ Returns: { calls_count: 0 }     │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │                         SERIALIZATION & RESPONSE PHASE                           │
 └─────────────────────────────────────────────────────────────────────────────────┘

                           ┌─────────────────────────────────┐
                           │ [TRACE 17-18] UpgradeFuture     │
                           │ Handler SUCCESS → serialize     │
                           │ GetServerStatisticsOutput →     │
                           │ HTTP Response                   │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼
                           ┌─────────────────────────────────┐
                           │ [TRACE A2] OUTER MIDDLEWARE     │
                           │ Response flows back through     │
                           │ Response Status: 200 OK         │
                           └──────────────┬──────────────────┘
                                          │
                                          ▼

      │                                                                    │
      │  HTTP/1.1 200 OK                                                   │
      │  Content-Type: application/json                                    │
      │  {"calls_count":0}                                                 │
      │ ◄───────────────────────────────────────────────────────────────── │
      │                                                                    │
    Client                                                              Server
```

## Type-Level Flow Summary

```
┌────────────────────────────────────────────────────────────────────────────────────┐
│                           TYPE-LEVEL SERVICE PIPELINE                              │
└────────────────────────────────────────────────────────────────────────────────────┘

 Hyper Connection
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  TowerToHyperService<IntoMakeService<RoutingService<RestRouter<Route>>>>        │
 │                                                                                  │
 │  Converts Tower Service interface to Hyper's expected interface                 │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  IntoMakeService<RoutingService<RestRouter<Route>>>                             │
 │                                                                                  │
 │  Service factory - clones inner service for each connection                     │
 │  File: routing/into_make_service.rs                                             │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  RoutingService<RestRouter<Route>, RestJson1>                                   │
 │                                                                                  │
 │  Main request dispatcher - matches requests to routes                           │
 │  File: routing/mod.rs                                                           │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  RestRouter<Route>                                                              │
 │                                                                                  │
 │  REST protocol router - matches method + path to Route                          │
 │  File: protocol/rest/router.rs                                                  │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  Route<B>                                                                       │
 │                                                                                  │
 │  Type-erased wrapper around the operation service                               │
 │  Wraps: BoxCloneService<Request<B>, Response<BoxBody>>                          │
 │  File: routing/route.rs                                                         │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  Upgrade<RestJson1, (GetServerStatisticsInput, (Extension<Arc<State>>,)), S>    │
 │                                                                                  │
 │  HTTP ↔ Smithy type conversion layer                                            │
 │  - Deserializes HTTP Request → Operation Input + Extensions                     │
 │  - Serializes Operation Output → HTTP Response                                  │
 │  File: operation/upgrade.rs                                                     │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  IntoService<GetServerStatistics, fn(Input, Extension<Arc<State>>) -> Output>   │
 │                                                                                  │
 │  Wraps the user's handler function as a Tower Service                           │
 │  File: operation/handler.rs                                                     │
 └─────────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │  User Handler: get_server_statistics()                                          │
 │                                                                                  │
 │  pub async fn get_server_statistics(                                            │
 │      _input: GetServerStatisticsInput,                                          │
 │      state: Extension<Arc<State>>,                                              │
 │  ) -> GetServerStatisticsOutput                                                 │
 │                                                                                  │
 │  File: pokemon-service-common/src/lib.rs                                        │
 └─────────────────────────────────────────────────────────────────────────────────┘
```

## Files Involved (in execution order)

| Order | Position | File | Type/Function | Purpose |
|-------|----------|------|---------------|---------|
| 1 | - | `serve/mod.rs` | `handle_connection()` | Accept TCP connection |
| 2 | - | `routing/into_make_service.rs` | `IntoMakeService::call()` | Clone router for connection |
| 3 | **A** | `outer_middleware.rs` | `OuterMiddlewareService::call()` | **Outer middleware** |
| 4 | - | `routing/mod.rs` | `RoutingService::call()` | Receive HTTP request |
| 5 | - | `protocol/rest/router.rs` | `RestRouter::match_route()` | Match route |
| 6 | **B** | `layer/alb_health_check.rs` | `AlbHealthCheckService::call()` | **Route middleware** |
| 7 | **B** | `request/request_id.rs` | `ServerRequestIdProvider::call()` | **Route middleware** |
| 8 | **B** | (tower_http) | `AddExtension::call()` | **Route middleware** |
| 9 | - | `routing/route.rs` | `Route::call()` | Forward to operation |
| 10 | **C** | `plugin.rs` | `PrintService::call()` | **HTTP plugin** (if scoped) |
| 11 | - | `operation/upgrade.rs` | `Upgrade::call()` | Start deserialization |
| 12 | - | `request/extension.rs` | `Extension::from_parts()` | Extract extensions |
| 13 | - | Generated `operation.rs` | `FromRequest::from_request()` | Deserialize body |
| 14 | **D** | `authz.rs` | `AuthorizeService::call()` | **Model plugin** |
| 15 | - | `operation/handler.rs` | `IntoService::call()` | Call handler |
| 16 | - | User handler | `get_server_statistics()` | Business logic |
| 17 | - | Generated `operation.rs` | `IntoResponse::into_response()` | Serialize response |
| 18 | **A** | `outer_middleware.rs` | Response flows back | **Outer middleware** |

## Middleware Configuration in pokemon-service

```rust
// main.rs - How middleware is configured:

// Position A: Outer Middleware (wraps entire app)
let outer_layer = OuterMiddlewareLayer::new();
let app = outer_layer.layer(app);

// Position B: Route Middleware (via config builder)
let config = PokemonServiceConfig::builder()
    .layer(AddExtensionLayer::new(Arc::new(State::default())))  // B
    .layer(AlbHealthCheckLayer::from_handler("/ping", ...))     // B
    .layer(ServerRequestIdProviderLayer::new())                 // B
    .http_plugin(http_plugins)   // Position C
    .model_plugin(model_plugins) // Position D
    .build();

// Position C: HTTP Plugins (operation-specific, on HTTP types)
let http_plugins = HttpPlugins::new()
    .push(Scoped::new::<PrintScope>(HttpPlugins::new().print()))
    .insert_operation_extension()
    .instrument();

// Position D: Model Plugins (operation-specific, on model types)
let model_plugins = ModelPlugins::new()
    .push(AuthorizationPlugin::new());
```

## Key Concepts

### Service Pipeline
The smithy-rs server uses a **nested Tower Service pipeline**. Each layer wraps the next, forming an onion-like structure. When a request comes in, it flows inward through each layer until reaching the handler, then the response flows outward through the same layers in reverse.

### Type Erasure
The `Route<B>` type uses `BoxCloneService` to erase the concrete operation service type. This allows the router to store a heterogeneous collection of operations with different input/output types in a single `Vec`.

### Upgrade Pattern
The `Upgrade` service is the bridge between the HTTP world (raw bytes) and the Smithy world (typed structs). It:
1. Deserializes the HTTP request using `FromRequest` (for body) and `FromParts` (for headers, extensions)
2. Calls the inner operation service with typed input
3. Serializes the typed output/error back to an HTTP response using `IntoResponse`

### Extensions
Extensions allow passing additional data (like shared state, connection info, request IDs) to handlers without modifying the Smithy model. They're extracted from the `http::Request::extensions()` bag using the `FromParts` trait.
