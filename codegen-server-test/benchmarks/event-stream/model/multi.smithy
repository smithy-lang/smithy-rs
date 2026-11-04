$version: "2.0"

// Declares the benchmark service with both REST JSON and rpcv2Cbor using the shared event
// operations. It exercises protocol selection while keeping handlers and workloads identical
// to the single-protocol benchmark services.

namespace benchmark.eventstream

@aws.protocols#restJson1
@smithy.protocols#rpcv2Cbor
service BenchmarkService {
    version: "1.0"
    operations: [Upload, Download, Echo]
}
