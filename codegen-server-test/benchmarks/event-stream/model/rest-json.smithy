$version: "2.0"

// Declares the REST JSON benchmark service over the shared upload, download, and duplex
// operations. Legacy and schema projections use this model to compare the same event workload.

namespace benchmark.eventstream

@aws.protocols#restJson1
service BenchmarkService {
    version: "1.0"
    operations: [Upload, Download, Echo]
}
