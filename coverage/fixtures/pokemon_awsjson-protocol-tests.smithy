$version: "2"
namespace coverage.fixtures.pokemon_awsjson

// Independent wire expectations follow the imported model bindings and populated values.
// Streaming event unions require dedicated frame fixtures; these traits do not waive failures.

apply com.aws.example#GetServerStatistics @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonAwsJsonStatisticsRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "params": {},
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0",
            "x-amz-target": "PokemonService.GetServerStatistics"
        },
        "body": "{}",
        "bodyMediaType": "application/json"
    }
])

apply com.aws.example#DoNothing @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonAwsJsonExplicitEmptyRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "params": {},
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0",
            "x-amz-target": "PokemonService.DoNothing"
        },
        "body": "{}",
        "bodyMediaType": "application/json"
    }
])

apply com.aws.example#CheckHealth @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonAwsJsonUnitRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#awsJson1_0",
        "method": "POST",
        "uri": "/",
        "params": {},
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0",
            "x-amz-target": "PokemonService.CheckHealth"
        },
        "body": "{}",
        "bodyMediaType": "application/json"
    }
])

apply com.aws.example#GetServerStatistics @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonAwsJsonStatisticsPopulatedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "params": {
            "calls_count": 42
        },
        "body": "{\"calls_count\":42}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        }
    },
    {
        "id": "CoveragePokemonAwsJsonStatisticsZeroResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "params": {
            "calls_count": 0
        },
        "body": "{\"calls_count\":0}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        }
    }
])

apply com.aws.example#DoNothing @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonAwsJsonExplicitEmptyResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "params": {},
        "body": "{}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        }
    }
])

apply com.aws.example#CheckHealth @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonAwsJsonUnitResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 200,
        "params": {},
        "body": "",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        }
    }
])

apply com.aws.example#UnsupportedRegionError @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonAwsJsonUnsupportedRegionResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 500,
        "params": {
            "region": "fixture-region"
        },
        "body": "{\"__type\":\"com.aws.example#UnsupportedRegionError\",\"region\":\"fixture-region\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        }
    }
])

apply com.aws.example#ThrottlingError @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonAwsJsonEmptyModeledErrorResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#awsJson1_0",
        "code": 400,
        "params": {},
        "body": "{\"__type\":\"com.aws.example#ThrottlingError\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/x-amz-json-1.0"
        }
    }
])
