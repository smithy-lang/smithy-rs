$version: "2.0"

namespace com.amazonaws.bignumbers

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

// Protocol tests for BigInteger and BigDecimal types.
//
// LIMITATION: Protocol test infrastructure has two precision constraints:
// 1. Smithy model parser converts numeric literals in `params` to Java Number (f64), losing precision
// 2. Protocol test validator uses serde_json which also truncates to f64
//
// Therefore these tests use:
// - BigInteger: 18446744073709551616 (u64::MAX + 1) - tests arbitrary precision for integers
// - BigDecimal: Values within f64 range - cannot test true arbitrary decimal precision here
//
// For comprehensive arbitrary precision testing including decimals > f64::MAX and high-precision
// decimals, see BigNumberPrecisionTest.kt integration tests which test actual serialization/
// deserialization without protocol test infrastructure limitations.

@restJson1
service BigNumberService {
    version: "2023-01-01"
    operations: [ProcessBigNumbers, ProcessNestedBigNumbers]
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
        body: "{\"bigInt\":18446744073709551616,\"bigDec\":123456.789}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: 18446744073709551616,
            bigDec: 123456.789
        }
    },
    {
        id: "ScientificNotationBigNumbersInJsonRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/process",
        body: "{\"bigInt\":12300000000,\"bigDec\":4.56e-5}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: 12300000000,
            bigDec: 0.0000456
        }
    },
    {
        id: "UppercaseScientificNotationBigNumbersInJsonRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/process",
        body: "{\"bigInt\":9870000000000000,\"bigDec\":3.21E-10}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            bigInt: 9870000000000000,
            bigDec: 0.000000000321
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
        body: "{\"result\":18446744073709551616,\"ratio\":123456.789}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: 18446744073709551616,
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
    },
    {
        id: "ScientificNotationBigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":1500000000000,\"ratio\":2.5E-8}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: 1500000000000,
            ratio: 0.000000025
        }
    },
    {
        id: "UppercaseScientificNotationBigNumbersInJsonResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"result\":789000000000000000000,\"ratio\":1.23E-15}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            result: 789000000000000000000,
            ratio: 0.00000000000000123
        }
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

// Collections and nested structures
list BigIntegerList {
    member: BigInteger
}

list BigDecimalList {
    member: BigDecimal
}

map StringToBigIntegerMap {
    key: String
    value: BigInteger
}

map StringToBigDecimalMap {
    key: String
    value: BigDecimal
}

structure NestedBigNumbers {
    numbers: BigIntegerList
    ratios: BigDecimalList
    intMap: StringToBigIntegerMap
    decMap: StringToBigDecimalMap
}

@http(uri: "/nested", method: "POST")
@httpRequestTests([
    {
        id: "BigNumbersInCollectionsRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/nested",
        body: "{\"numbers\":[1,2,3],\"ratios\":[1.1,2.2,3.3],\"intMap\":{\"a\":100,\"b\":200},\"decMap\":{\"x\":0.5,\"y\":1.5}}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            numbers: [1, 2, 3],
            ratios: [1.1, 2.2, 3.3],
            intMap: {a: 100, b: 200},
            decMap: {x: 0.5, y: 1.5}
        }
    },
    {
        id: "LargeBigNumbersInCollectionsRequest",
        protocol: restJson1,
        method: "POST",
        uri: "/nested",
        body: "{\"numbers\":[18446744073709551616],\"ratios\":[123456.789],\"intMap\":{\"big\":18446744073709551616},\"decMap\":{\"precise\":0.123456789}}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            numbers: [18446744073709551616],
            ratios: [123456.789],
            intMap: {big: 18446744073709551616},
            decMap: {precise: 0.123456789}
        }
    }
])
@httpResponseTests([
    {
        id: "BigNumbersInCollectionsResponse",
        protocol: restJson1,
        code: 200,
        body: "{\"numbers\":[10,20,30],\"ratios\":[0.1,0.2,0.3],\"intMap\":{\"x\":1000},\"decMap\":{\"y\":99.99}}",
        bodyMediaType: "application/json",
        headers: {"Content-Type": "application/json"},
        params: {
            numbers: [10, 20, 30],
            ratios: [0.1, 0.2, 0.3],
            intMap: {x: 1000},
            decMap: {y: 99.99}
        }
    }
])
operation ProcessNestedBigNumbers {
    input: NestedBigNumbers
    output: NestedBigNumbers
}
