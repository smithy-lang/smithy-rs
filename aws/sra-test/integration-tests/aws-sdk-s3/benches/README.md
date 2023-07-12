### Middleware vs. Orchestrator Benchmark

To run the benchmark:
```bash
./gradlew :aws:sra-test:assemble && (cd aws/sra-test/integration-tests/aws-sdk-s3 && cargo bench)
```
