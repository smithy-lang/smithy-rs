S3 Benchmark
============

This directory contains a S3 benchmark that measures throughput when using the AWS Rust SDK to put and get objects to/from S3.
The `benchmark/` directory has the Rust benchmark code, and `infrastructure/` contains the CDK infrastructure to stand up
a `c5n.18xlarge` EC2 instance, compile/run the benchmark, and upload the results to S3.

Example of running the `get-object-multipart` benchmark in local dev:

```bash
cargo run -- --bench get-object-multipart --fs disk --part-size-bytes 5242880 --size-bytes 6000000 --bucket my-test-bucket --region us-west-2 --profile my-aws-credentials-profile
```

On Linux, the `--fs` option can be given either `disk` or `tmpfs` (for an in-memory filesystem), while on other OSes, only `disk` is available.

In addition to `get-object-multipart`, there are `put-object-multipart`, `put-object`, and `get-object`. All of these take the
same CLI arguments, although `--part-size-bytes` is unused by `put-object` and `get-object`.

To run the actual benchmark, it must be deployed via CDK from the `infrastructure/` directory:

```bash
npm install
npm run build
npx cdk bootstrap --profile my-aws-credentials-profile
npx cdk synthesize --profile my-aws-credentials-profile
npx cdk deploy --profile my-aws-credentials-profile
```

The `lib/instrastructure-stack.ts` defines the actual CloudFormation stack that creates the EC2 Instance.
This instance is configured to run the `assets/init_instance.sh` and `assets/run_benchmark.sh` scripts on start-up.
It's also configured for SSH access via a key pair named "S3BenchmarkKeyPair". This key pair has to be created manually
before deploying the CDK stack.
