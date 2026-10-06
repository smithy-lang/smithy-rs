Repeatable server routing benchmark
===================================

Run from the repository root:

  python3 tools/server-routing-benchmark/prepare.py
  python3 tools/server-routing-benchmark/build.py
  python3 tools/server-routing-benchmark/run.py

prepare.py generates five baseline servers from ~/smithy-rs-latest, each with
one protocol, and one schema server from this checkout with all five protocols.
The same eight-operation model is used, with only protocol traits changed.
Generated Rust, model variants and generation metadata are retained under
artifacts/. A completed generation is reused on subsequent invocations.
Use a new --cache directory to explicitly generate a new baseline/model version.
No existing source model in either checkout is modified.

build.py builds CPU and memory executables against the retained servers and each
checkout's local runtime. It never invokes codegen. After changing runtime code,
run build.py again, then run.py. Cargo rebuilds the changed dependencies.

run.py only runs existing binaries. It never generates or compiles anything.
It rejects stale runtime/harness builds; rerun build.py when prompted.
Each run writes a new timestamped results directory containing raw JSON samples,
a CSV/JSON summary, revisions, checkout diffs, runtime source hashes, binary
hashes, compiler details and machine information. Both checkouts may have local
changes: this is a working-tree comparison, not a comparison of clean releases.

Measurement
-----------
* Eight registered operations; requests invoke Echo and return the supplied value.
* restJson1, restXml, awsJson1_0, awsJson1_1 and rpcv2Cbor.
* Values contain 64, 1024 or 16384 ASCII bytes; wire bodies include their envelopes.
* Full request pipeline: build HTTP request, clone the service, readiness/call,
  routing, body read, deserialization, handler, serialization, response collection.
* Input bodies are http_body_util::Full<Bytes>. This includes each implementation's
  body adaptation costs. These are in-process results, not Hyper Incoming/socket
  throughput measurements; there is no HTTP parser, network, TLS, or middleware.
* One current-thread Tokio runtime; sequential requests; process pinned to one
  allowed logical CPU (override with --cpu). No isolated host or CPU frequency lock.
* Responses are checked before measurement for status and exact echoed field.
* 300 ms warmup, then 40 calibrated batches targeting 20 ms each. Process CPU time
  and wall time are recorded separately. Three process runs per case, alternating
  baseline/current order. Summary reports median of run medians and min/max run
  medians; these ranges are not confidence intervals.
* CPU binaries use the normal system allocator. Separate memory binaries count
  allocation/reallocation calls, requested bytes, and peak additional live heap
  over 1000 warmed requests. They do not measure allocator metadata or stack use.
* Peak RSS comes from /usr/bin/time on each uninstrumented process, including
  startup and warmup. It is whole-process resident memory, not per-request memory.
  Binary layout and shared-library pages affect small RSS differences.
* Request buffers, input strings and runtime setup are outside measurement;
  request construction and response draining are inside measurement.
* The baseline uses its own Tower/runtime versions; current uses its own versions.
  Differences cannot be attributed solely to routing or Arc cloning.

The input string is optional in the model, though every request supplies it and
the handler requires it. The baseline REST XML generator failed to compile the
required-input-string form (set_value received Option<String>); no generated
baseline code or generator was patched to hide that failure.

Useful options
--------------
prepare.py: --baseline PATH --current PATH --cache PATH
build.py:   --cache PATH
run.py:     --cache PATH --cpu N --repeats N --sizes 64 1024 16384 --output PATH
            --protocols rest_json rest_xml aws_json_10 aws_json_11 rpc_v2_cbor

Generated sources, Cargo locks, binaries, and raw results are local artifacts
ignored by Git and persist across benchmark runs. Do not remove artifacts/ if
you want to preserve the generated servers. The baseline checkout is not copied:
its runtime source must remain available at the recorded path.
