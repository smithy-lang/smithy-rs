# Pokemon Service Benchmarks

Run these commands from `examples-static/`.

## CPU

```bash
cargo bench -p pokemon-service-benchmarks --bench get_server_statistics
```

For quicker local comparisons:

```bash
cargo bench -p pokemon-service-benchmarks --bench get_server_statistics -- --sample-size 10
```

## Allocations

```bash
cargo run -p pokemon-service-benchmarks --release --bin alloc_get_server_statistics
```

The allocation report prints CSV rows for all 15 cases:

- `legacy`, `dynamic`, and `static`
- `restJson1`, `restXml`, `awsJson1_0`, `awsJson1_1`, and `rpcv2Cbor`

Columns are `alloc_count`, `dealloc_count`, `allocated_bytes`, and `peak_live_bytes`.

## Flamegraphs

The profiler binary runs exactly one selected case in a tight loop. The stable case names match the Criterion benchmark names, for example:

```bash
cargo flamegraph --root --package pokemon-service-benchmarks --bin profile_get_server_statistics -- rpcv2Cbor/legacy
cargo flamegraph --root --package pokemon-service-benchmarks --bin profile_get_server_statistics -- rpcv2Cbor/dynamic
cargo flamegraph --root --package pokemon-service-benchmarks --bin profile_get_server_statistics -- rpcv2Cbor/static
```

If `--root` is not usable, run without it and note that kernel symbols may be missing:

```bash
cargo flamegraph --package pokemon-service-benchmarks --bin profile_get_server_statistics -- rpcv2Cbor/static
```

Set `POKEMON_BENCH_ITERS` to override the profiler loop count:

```bash
POKEMON_BENCH_ITERS=10000 cargo run -p pokemon-service-benchmarks --release --bin profile_get_server_statistics -- rpcv2Cbor/static
```
