$version: "2.0"
namespace benchmark.eventstream

@smithy.protocols#rpcv2Cbor
service BenchmarkService {
    version: "1.0"
    operations: [Upload, Download, Echo]
}
