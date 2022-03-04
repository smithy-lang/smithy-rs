# Smithy Rust Server SDK benchmarks

This Pokémon Service has been benchmarked on different type of EC2 instances
using [wrk](https://github.com/wg/wrk).

<!-- vim-markdown-toc Marked -->

* [2022-03-04](#2022-03-04)
    * [c6i.8xlarge](#c6i.8xlarge)
        * [Full result](#full-result)
    * [c6g.8xlarge](#c6g.8xlarge)
        * [Full result](#full-result)

<!-- vim-markdown-toc -->

## 2022-03-04

The benchmark run against the `get_pokemon_species()` operation, which is
reading the Pokémon specie translation from an in memory hashmap and increasing
an atomic counter. This operation performs both deserialization of input and
serialization of output.

### c6i.8xlarge

* 32 cores Intel(R) Xeon(R) Platinum 8375C CPU @ 2.90GHz
* 64 Gb memory
* Benchmark:
    - Duration: 10 minutes
    - Connections: 512
    * Threads: 64
* Result:
    - Request/sec: 1068053
    * RSS memory: 39900 bytes

#### Full result

```
❯❯❯ wrk -d 10m -c 512 -t 64 --latency http://localhost:13734/pokemon-species/pikachu
Running 10m test @ http://localhost:13734/pokemon-species/pikachu
  64 threads and 512 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   485.78us  237.79us  33.22ms   78.98%
    Req/Sec    16.77k   272.59    62.96k    79.93%
  Latency Distribution
     50%  459.00us
     75%  590.00us
     90%  738.00us
     99%    1.13ms
  640938431 requests in 10.00m, 313.98GB read
Requests/sec: 1068053.32
Transfer/sec:    535.77MB
```

### c6g.8xlarge

* 32 cores Amazon Graviton 2 @ 2.50GHz
* 64 Gb memory
* Benchmark:
    - Duration: 10 minutes
    - Connections: 512
    * Threads: 64
* Result:
    - Request/sec: 791008
    * RSS memory: 41540 bytes


#### Full result

```
❯❯❯ wrk -d 10m -c 512 -t 64 --latency http://localhost:13734/pokemon-species/pikachu
Running 10m test @ http://localhost:13734/pokemon-species/pikachu
  64 threads and 512 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   656.05us  324.72us  23.38ms   77.51%
    Req/Sec    12.42k   297.47    48.07k    74.52%
  Latency Distribution
     50%  618.00us
     75%  805.00us
     90%    1.01ms
     99%    1.55ms
  474684091 requests in 10.00m, 232.54GB read
Requests/sec: 791008.58
Transfer/sec:    396.80MB
```
