# DynamoDB Hello World Example
This repo has a simple hello-world example for DynamoDB that will create a table if it doesn't exist & list tables present in the database.

By default, the code is written to target DynamoDB local—A docker compose file is provided for convenience. Usage:

```
docker-compose up -d
cargo run
```

## Running against real DynamoDB

This hasn't been tested, but you'd need to:
- Remove the endpoint provider override
- Set real credentials (currently only static credentials are supported)
