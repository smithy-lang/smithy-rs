$version: "2.0"


namespace aws.protocoltests.json

use aws.protocols#awsJson1_0
use smithy.test#httpResponseTests

@awsJson1_0
service RequiredValue {
    operations: [SayHello],
    version: "1"
}

operation SayHello { output: TestOutput }

structure TestOutput with [TestStruct] { innerField: Nested }

@mixin
structure TestStruct {
    @required
    foo: String,
    @required
    byteValue: Byte,
    @required
    listValue: StringList,
    @required
    mapValue: ListMap,
    @required
    doubleListValue: DoubleList
    @required
    document: Document
    @required
    nested: Nested
    @required
    blob: Blob
    @required
    enum: Enum
    @required
    union: U
    notRequired: String
}

enum Enum {
    A,
    B,
    C
}
union U {
    A: Integer,
    B: String,
    C: Unit
}

structure Nested {
    @required
    a: String
}

list StringList {
    member: String
}

list DoubleList {
    member: StringList
}

map ListMap {
    key: String,
    value: StringList
}

apply SayHello @httpResponseTests([{
                                         id: "error_recovery",
                                         protocol: awsJson1_0,
                                         params: {
                                             union: { A: 5 },
                                             enum: "A",
                                             foo: "",
                                             byteValue: 0,
                                             blob: "",
                                             listValue: [],
                                             mapValue: {},
                                             doubleListValue: []
                                             document: null
                                             nested: { a: "" }
                                         },
                                         code: 200,
                                         body: "{}"
                                     }])
