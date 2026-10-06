// coverage-service: test.bodylimit#BodyLimitService
// coverage-plugins: server
// coverage-codegen: "requestBodyMaxBytes": 1048576
$version: "2.0"

namespace test.bodylimit

use aws.protocols#restJson1

@restJson1
service BodyLimitService {
    version: "1.0"
    operations: [PutDocumentBody, PutBlobPayload, PutStringPayload]
}

@http(method: "POST", uri: "/doc")
operation PutDocumentBody {
    input := {
        name: String
        count: Integer
    }
    output := {
        ok: Boolean
    }
}

@http(method: "POST", uri: "/blob")
operation PutBlobPayload {
    input := {
        @httpPayload
        data: Blob
    }
    output := {
        ok: Boolean
    }
}

@http(method: "POST", uri: "/text")
operation PutStringPayload {
    input := {
        @httpPayload
        text: String
    }
    output := {
        ok: Boolean
    }
}
