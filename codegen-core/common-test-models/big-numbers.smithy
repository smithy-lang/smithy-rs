$version: "2.0"

namespace com.amazonaws.bignumbers

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
service BigNumberService {
    version: "2023-01-01"
    operations: [ProcessBigNumbers]
}

@http(uri: "/process", method: "POST")
@httpRequestTests([
    {
        id: "BigNumbersInJsonRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/process",
        body: "{\"bigInt\":123456789,\"bigDec\":123.456789}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: 123456789,
            bigDec: 123.456789
        }
    },
    {
        id: "NegativeBigNumbersInJsonRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/process",
        body: "{\"bigInt\":-987654321,\"bigDec\":-0.000000001}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: -987654321,
            bigDec: -0.000000001
        }
    },
    {
        id: "ZeroBigNumbersInJsonRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/process",
        body: "{\"bigInt\":0,\"bigDec\":0.0}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: 0,
            bigDec: 0.0
        }
    },
    {
        id: "VeryLargeBigNumbersInJsonRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/process",
        body: "{\"bigInt\":9007199254740991,\"bigDec\":123456.789}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: 9007199254740991,
            bigDec: 123456.789
        }
    }
])
@httpResponseTests([
    {
        id: "BigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":999999999,\"ratio\":0.123456789}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: 999999999,
            ratio: 0.123456789
        }
    },
    {
        id: "NegativeBigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":-123456789,\"ratio\":-999.999}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: -123456789,
            ratio: -999.999
        }
    },
    {
        id: "VeryLargeBigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":9007199254740991,\"ratio\":123456.789}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: 9007199254740991,
            ratio: 123456.789
        }
    },
    {
        id: "ZeroBigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":0,\"ratio\":0.0}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: 0,
            ratio: 0.0
        }
    },
    {
        id: "NullBigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":null,\"ratio\":null}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {}
    }
])

operation ProcessBigNumbers {
    input: BigNumberInput
    output: BigNumberOutput
}

structure BigNumberInput {
    bigInt: BigInteger
    bigDec: BigDecimal
}

structure BigNumberOutput {
    result: BigInteger
    ratio: BigDecimal
}
