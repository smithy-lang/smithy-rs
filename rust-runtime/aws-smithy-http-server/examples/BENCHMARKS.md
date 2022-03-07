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

## [2022-03-04](https://github.com/awslabs/smithy-rs/commit/d823f61156577ab42590709627906d1dc35a5f49)

The benchmark runs against the `empty_operation()` operation, which is just
returning an empty output and can be used to stress test the framework overhead.

### c6i.8xlarge

* 32 cores Intel(R) Xeon(R) Platinum 8375C CPU @ 2.90GHz
* 64 Gb memory
* Benchmark:
    - Duration: 10 minutes
    - Connections: 1024
    - Threads: 16
* Result:
    - Request/sec: 1_608_742
    * RSS[^1] memory: 72200 bytes

#### Full result

```
❯❯❯ wrk -t16 -c1024 -d10m --latency http://localhost:13734/empty-operation
Running 10m test @ http://localhost:13734/empty-operation
  16 threads and 1024 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency     1.03ms    1.84ms 208.10ms   92.16%
    Req/Sec   101.11k    17.59k  164.78k    70.99%
  Latency Distribution
     50%  475.00us
     75%  784.00us
     90%    2.12ms
     99%    9.74ms
  965396910 requests in 10.00m, 98.00GB read
  Socket errors: connect 19, read 0, write 0, timeout 0
Requests/sec: 1608742.65
Transfer/sec:    167.23MB
```

### c6g.8xlarge

* 32 cores Amazon Graviton 2 @ 2.50GHz
* 64 Gb memory
* Benchmark:
    - Duration: 10 minutes
    - Connections: 1024
    - Threads: 16
* Result:
    - Request/sec: 1_379_942
    - RSS[^1] memory: 70264 bytes


#### Full result

```
❯❯❯ wrk -t16 -c1024 -d10m --latency http://localhost:13734/empty-operation
Running 10m test @ http://localhost:13734/empty-operation
  16 threads and 1024 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency     1.26ms    2.22ms 210.68ms   91.99%
    Req/Sec    86.76k    16.46k  141.30k    68.81%
  Latency Distribution
     50%  560.00us
     75%    0.93ms
     90%    2.53ms
     99%   11.95ms
  828097344 requests in 10.00m, 84.06GB read
  Socket errors: connect 19, read 0, write 0, timeout 0
Requests/sec: 1379942.45
Transfer/sec:    143.45MB
```

[^1]: https://en.wikipedia.org/wiki/Resident_set_size
