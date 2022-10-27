AWS auth needs metrics

current client is dynamodb

need tracing

pass the metrics system to the rust plugin
like we do for layers of metrics in SmithyRS

rustquerylog is the only one that does metrics


tracing -> opentelemetry
opentelemetry -> opentelemetry collector (HoneyComb, OTLP?)


3 things in the ecosystem right now
  - metrics-rs/metrics (most complete)
      - Might not support sampling (do stuff every x% of requests)
      - Counters, gauges, histograms
  - open-telemetry
    spec not done yet
  - tracing
    do a layer manually on top of it

Internal package: Rust-querylog

read: https://www.lpalmieri.com/posts/2020-09-27-zero-to-production-4-are-we-observable-yet/
