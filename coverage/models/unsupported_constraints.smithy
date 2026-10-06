// coverage-service: test.unsupported#UnsupportedConstraintsService
// coverage-plugins: server
// coverage-codegen: "ignoreUnsupportedConstraints": true
$version: "2.0"

namespace test.unsupported

use aws.protocols#restJson1
use smithy.framework#ValidationException

@restJson1
service UnsupportedConstraintsService {
    version: "1.0"
    operations: [PutStuff, UploadStream]
}

@http(method: "POST", uri: "/stuff")
operation PutStuff {
    input := {
        // constraint traits directly on member shapes are unsupported
        @length(min: 1, max: 10)
        memberConstrainedString: String

        @range(min: 1, max: 5)
        memberConstrainedInt: Integer

        @pattern("^[a-z]+$")
        memberConstrainedPattern: String

        // @range on floating point shapes is unsupported
        ratio: ConstrainedFloat

        bigRatio: ConstrainedDouble
    }
    output := {
        ok: Boolean
    }
    errors: [ValidationException]
}

@range(min: 0, max: 1)
float ConstrainedFloat

@range(min: 0, max: 100)
double ConstrainedDouble

@http(method: "POST", uri: "/stream")
operation UploadStream {
    input := {
        @required
        @httpPayload
        data: SizedStreamingBlob
    }
    output := {
        ok: Boolean
    }
}

// @length on a streaming blob is unsupported
@length(min: 1, max: 1024)
@streaming
blob SizedStreamingBlob
