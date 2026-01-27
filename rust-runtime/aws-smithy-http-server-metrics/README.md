# AWS Smithy HTTP Server Metrics

Metrics collection for Smithy Rust servers.

## Default Metrics

For out-of-the-box default metrics, the `DefaultMetricsPlugin` is provided to add to the http plugins of a smithy-rs service.

e.g.

```rust
let http_plugins = HttpPlugins::new().push(DefaultMetricsPlugin);
let config = PokemonServiceConfig::builder().http_plugin(http_plugins).build();
PokemonService::builder(config)
...
```

The following metrics will be automatically collected for every request:

### Request Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `service_name` | Property | Name of the service |
| `service_version` | Property | Version of the service |
| `operation_name` | Property | Name of the operation being invoked |
| `request_id` | Property | Unique identifier for the request |

### Response Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `http_status_code` | Property | HTTP status code of the response |
| `error` | Counter | Client error indicator (4xx status code) |
| `fault` | Counter | Server fault indicator (5xx status code) |
| `operation_time` | Duration | Wallclock time from pre-deserialization of the model input to post-serialization of the model output |

## Defining Custom Metrics

Use the `#[smithy_metrics]` macro to integrate a metrique struct with .

Use the `#[smithy_metrics(extension)]` on struct fields to make them available in operation handlers.

```rust
#[smithy_metrics]
#[metrics]
#[derive(Default)]
pub struct PokemonMetrics {
    #[metrics(flatten)]
    pub request_metrics: PokemonRequestMetrics,
    #[metrics(flatten)]
    pub response_metrics: PokemonResponseMetrics,
    #[metrics(flatten)]
    #[smithy_metrics(extension)]
    pub operation_metrics: PokemonOperationMetrics,
}
```

See the [pokemon-service example](../../examples/pokemon-service-common/src/metrics.rs) for a complete example implementation.

## Output Formats

### CloudWatch EMF (Embedded Metric Format)

Metrics are emitted as structured JSON logs that CloudWatch automatically converts to metrics:

```json
{
  "_aws": {
    "Timestamp": 1737847711257,
    "CloudWatchMetrics": [{
      "Namespace": "PokemonService",
      "Dimensions": [[]],
      "Metrics": [
        {"Name": "error"},
        {"Name": "fault"},
        {"Name": "found"},
        {"Name": "activity_time", "Unit": "Milliseconds"},
        {"Name": "total_time", "Unit": "Milliseconds"}
      ]
    }]
  },
  "error": 0,
  "fault": 0,
  "found": 1,
  "activity_time": 12.847,
  "total_time": 15.392,
  "requested_pokemon_name": "pikachu",
  "service_name": "PokemonService",
  "service_version": "2024-03-18",
  "operation_name": "GetPokemonSpecies",
  "request_id": "01JKXM7N8QZFP2VWXYZ123ABC",
  "http_status_code": "200"
}
```

**Structure**:
- `_aws.CloudWatchMetrics`: Metadata for CloudWatch metric extraction
- `_aws.Timestamp`: Unix timestamp in milliseconds
- `Namespace`: CloudWatch namespace for grouping metrics
- `Dimensions`: Dimensions for filtering (empty array = no dimensions)
- `Metrics`: List of metric names and units
- Root-level fields: Actual metric values and properties

### Standard Log Output

When not using CloudWatch, metrics appear as key-value pairs:

```
service_name=PokemonService service_version=2024-03-18 operation_name=GetPokemonSpecies request_id=abc123 total_time=45.23ms http_status_code=200 requested_pokemon_name=pikachu found=1 activity_time=42.15ms
```

## Examples

See the [pokemon-service example](../../examples/pokemon-service-common/src/metrics.rs) for a complete implementation.
