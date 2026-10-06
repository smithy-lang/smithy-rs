// coverage-service: test.sensitive#SensitiveService
// coverage-plugins: server
$version: "2.0"

namespace test.sensitive

use aws.protocols#restJson1

@restJson1
service SensitiveService {
    version: "1.0"
    operations: [GetSecret, PutSecret, ProxySecret, QuerySecret]
}

// greedy label whose value is sensitive
@http(method: "GET", uri: "/proxy/{path+}/end")
operation ProxySecret {
    input := {
        @required
        @httpLabel
        path: SensitiveString
    }
    output := {
        @httpResponseCode
        code: SensitiveInt
    }
}

// sensitive @httpQueryParams map and sensitive prefix-header keys
@http(method: "GET", uri: "/query-secret")
operation QuerySecret {
    input := {
        @httpQueryParams
        params: SensitiveValueMap

        @httpPrefixHeaders("x-key-")
        keyed: SensitiveKeyMap
    }
    output := {
        ok: Boolean
    }
}

@http(method: "GET", uri: "/secret/{secretId}")
operation GetSecret {
    input := {
        @required
        @httpLabel
        secretId: SensitiveString

        @httpQuery("token")
        token: SensitiveString

        @httpHeader("x-auth")
        authHeader: SensitiveString

        @httpPrefixHeaders("x-meta-")
        meta: SensitiveHeaderMap
    }
    output := {
        @httpHeader("x-secret-value")
        value: SensitiveString

        @httpPayload
        body: SensitivePayload
    }
}

@http(method: "POST", uri: "/secret")
operation PutSecret {
    input := {
        @httpPayload
        body: SensitiveBlob
    }
    output := {
        @httpResponseCode
        code: Integer

        secret: SensitiveString
        nested: NestedSensitive
    }
}

@sensitive
string SensitiveString

@sensitive
integer SensitiveInt

// map where values are sensitive
map SensitiveValueMap {
    key: String
    value: SensitiveString
}

// map where keys are sensitive
map SensitiveKeyMap {
    key: SensitiveString
    value: String
}

@sensitive
blob SensitiveBlob

map SensitiveHeaderMap {
    key: String
    value: SensitiveString
}

@sensitive
structure SensitivePayload {
    name: String
    value: String
}

structure NestedSensitive {
    inner: SensitiveString
    plain: String
}
