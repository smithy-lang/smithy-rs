// coverage-service: test.auth#AuthService
// coverage-plugins: client
$version: "2.0"

namespace test.auth

use aws.protocols#restJson1

@restJson1
@httpApiKeyAuth(name: "x-api-key", in: "header")
@httpBasicAuth
@httpBearerAuth
@httpDigestAuth
@auth([httpApiKeyAuth, httpBasicAuth, httpBearerAuth, httpDigestAuth])
service AuthService {
    version: "1.0"
    operations: [DefaultAuthOp, BasicOnlyOp, OptionalAuthOp, NoAuthOp]
}

@http(method: "GET", uri: "/default")
operation DefaultAuthOp {}

@auth([httpBasicAuth])
@http(method: "GET", uri: "/basic")
operation BasicOnlyOp {}

@optionalAuth
@http(method: "GET", uri: "/optional")
operation OptionalAuthOp {}

@auth([])
@http(method: "GET", uri: "/none")
operation NoAuthOp {}
