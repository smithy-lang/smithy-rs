$version: "2.0"

// Declares the rpcv2Cbor benchmark service over the shared upload, download, and duplex
// operations. The benchmark and wire tests check CBOR event payloads and initial messages.

namespace benchmark.eventstream

@smithy.protocols#rpcv2Cbor
service BenchmarkService {
    version: "1.0"
    operations: [Upload, Download, Echo]
}
