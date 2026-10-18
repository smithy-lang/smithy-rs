$version: "2.0"
namespace benchmark.eventstream

@aws.protocols#restJson1
service BenchmarkService {
    version: "1.0"
    operations: [Upload, Download, Echo]
}
