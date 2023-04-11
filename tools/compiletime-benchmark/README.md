# docker

This is meant to be uploaded to amazon ecr's docker repository.
It will be pulled into the EC2 instance that batch prepared for you.

## Env variables
- TARGET_COMMIT_HASH  
  This is the commit hash which the benchmark will run against.
- S3_BUCKET  
  This is the bucket that result is going to uploaded.
- S3_PREFIX
  Uploaded object will be prefixed with `{S3_PREFIX}/{COMMIT_HASH}`
