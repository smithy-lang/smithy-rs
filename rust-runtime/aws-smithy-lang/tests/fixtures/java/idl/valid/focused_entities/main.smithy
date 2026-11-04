$version: "2"
namespace ex

service Service {
    version: "2024-01-01"
    operations: [Ping]
    resources: [City]
    errors: [ServiceError]
    rename: {
        "other.ns#Name": "OtherName"
    }
}

resource City {
    identifiers: { cityId: CityId }
    properties: { name: Name }
    create: CreateCity
    put: PutCity
    read: GetCity
    update: UpdateCity
    delete: DeleteCity
    list: ListCities
    operations: [Touch]
    collectionOperations: [Batch]
    resources: [Forecast]
}

resource Forecast {
    identifiers: { cityId: CityId }
}

string CityId

operation Ping {
    input := {
        name: Name
        other: other.ns#Name
    }
}

operation CreateCity {
    input := {
        name: Name
    }
    output := {
        @required
        cityId: CityId
    }
}


@idempotent
operation PutCity {
    input := {
        @required
        cityId: CityId
        name: Name
    }
}

@readonly
operation GetCity {
    input := {
        @required
        cityId: CityId
    }
    output := {
        name: Name
    }
}

operation UpdateCity {
    input := {
        @required
        cityId: CityId
    }
}

@idempotent
operation DeleteCity {
    input := {
        @required
        cityId: CityId
    }
}

@readonly
operation ListCities {}

operation Touch {
    input := {
        @required
        cityId: CityId
    }
}

operation Batch {}

string Name

@error("server")
structure ServiceError {}
