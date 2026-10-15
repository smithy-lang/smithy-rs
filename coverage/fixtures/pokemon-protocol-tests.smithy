$version: "2"
namespace coverage.fixtures.pokemon

// Independent wire expectations follow the imported model bindings and populated values.
// Streaming event unions require dedicated frame fixtures; these traits do not waive failures.

apply com.aws.example#GetPokemonSpecies @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonSpeciesLabelRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/pokemon-species/pikachu",
        "params": {
            "name": "pikachu"
        },
        "appliesTo": "server",
        "body": ""
    },
    {
        "id": "CoveragePokemonSpeciesEscapedLabelRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/pokemon-species/mr%20mime",
        "params": {
            "name": "mr mime"
        },
        "appliesTo": "server",
        "body": ""
    }
])

apply com.aws.example#GetPokemonSpecies @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonSpeciesPopulatedCollectionResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "name": "pikachu",
            "flavorTextEntries": [
                {
                    "flavorText": "electric mouse",
                    "language": "en"
                },
                {
                    "flavorText": "ratón eléctrico",
                    "language": "es"
                }
            ]
        },
        "body": "{\"name\":\"pikachu\",\"flavorTextEntries\":[{\"flavorText\":\"electric mouse\",\"language\":\"en\"},{\"flavorText\":\"ratón eléctrico\",\"language\":\"es\"}]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    },
    {
        "id": "CoveragePokemonSpeciesEmptyRequiredCollectionResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "name": "eevee",
            "flavorTextEntries": []
        },
        "body": "{\"name\":\"eevee\",\"flavorTextEntries\":[]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.aws.example#GetStorage @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonStorageLabelHeaderRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/pokedex/trainer",
        "params": {
            "user": "trainer",
            "passcode": "fixture-passcode"
        },
        "appliesTo": "server",
        "headers": {
            "passcode": "fixture-passcode"
        },
        "body": ""
    }
])

apply com.aws.example#GetStorage @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonStoragePopulatedCollectionResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "collection": [
                "pikachu",
                "eevee"
            ]
        },
        "body": "{\"collection\":[\"pikachu\",\"eevee\"]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    },
    {
        "id": "CoveragePokemonStorageEmptyRequiredCollectionResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "collection": []
        },
        "body": "{\"collection\":[]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.aws.example#GetServerStatistics @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonStatisticsEmptyRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/stats",
        "params": {},
        "appliesTo": "server",
        "body": ""
    }
])

apply com.aws.example#GetServerStatistics @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonStatisticsPopulatedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "calls_count": 42
        },
        "body": "{\"calls_count\":42}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    },
    {
        "id": "CoveragePokemonStatisticsZeroResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "calls_count": 0
        },
        "body": "{\"calls_count\":0}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.aws.example#DoNothing @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonExplicitEmptyRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/do-nothing",
        "params": {},
        "appliesTo": "server",
        "body": ""
    }
])

apply com.aws.example#DoNothing @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonExplicitEmptyResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {},
        "body": "{}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.aws.example#CheckHealth @smithy.test#httpRequestTests([
    {
        "id": "CoveragePokemonUnitRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/ping",
        "params": {},
        "appliesTo": "server",
        "body": ""
    }
])

apply com.aws.example#CheckHealth @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonUnitResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {},
        "body": "",
        "bodyMediaType": "application/json",
        "appliesTo": "server"
    }
])

apply com.aws.example#StreamPokemonRadio @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonRadioPopulatedStreamingBytesResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "data": "radio-fixture"
        },
        "body": "radio-fixture",
        "bodyMediaType": "application/octet-stream",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/octet-stream"
        }
    },
    {
        "id": "CoveragePokemonRadioDefaultEmptyBytesResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {},
        "body": "",
        "bodyMediaType": "application/octet-stream",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/octet-stream"
        }
    }
])

apply com.aws.example#ResourceNotFoundException @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonResourceNotFoundResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 404,
        "params": {
            "message": "missing species"
        },
        "body": "{\"message\":\"missing species\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json",
            "x-amzn-errortype": "ResourceNotFoundException"
        }
    }
])

apply com.aws.example#StorageAccessNotAuthorized @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonStorageUnauthorizedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 401,
        "params": {},
        "body": "{}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json",
            "x-amzn-errortype": "StorageAccessNotAuthorized"
        }
    }
])

apply com.aws.example#UnsupportedRegionError @smithy.test#httpResponseTests([
    {
        "id": "CoveragePokemonUnsupportedRegionResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 500,
        "params": {
            "region": "fixture-region"
        },
        "body": "{\"region\":\"fixture-region\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json",
            "x-amzn-errortype": "UnsupportedRegionError"
        }
    }
])
