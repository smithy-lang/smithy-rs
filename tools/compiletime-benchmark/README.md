## Rust Code
It sets up necessary resources on AWS.  
`./serde-aws-sdk` directory has a aws code with serde implemented.  
This is here because RFC30 is not merged into the main branch yet.  

Once it is merged I think we can remove the subdirectory.  

## docker

This is meant to be uploaded to amazon ecr's docker repository.
It will be pulled into the EC2 instance that batch prepared for you.

### Env variables
- TARGET_COMMIT_HASH  
  This is the commit hash which the benchmark will run against.
- S3_BUCKET  
  This is the bucket that result is going to uploaded.
- S3_PREFIX
  Uploaded object will be prefixed with `{S3_PREFIX}/{COMMIT_HASH}`
