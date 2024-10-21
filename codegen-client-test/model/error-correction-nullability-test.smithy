$version: "2.0"


namespace aws.protocoltests.json

use aws.protocols#awsJson1_0
use aws.protocols#restXml
use smithy.test#httpResponseTests

@awsJson1_0
service RequiredValueJson {
    operations: [SayHello],
    version: "1"
}


@restXml
service RequiredValueXml {
    operations: [SayHelloXml],
    version: "1"
}

@error("client")
structure Error {
    @required
    requestId: String

    @required
    message: String
}

@http(method: "POST", uri: "/")
operation SayHello { output: TestOutputDocument, errors: [Error] }

@http(method: "POST", uri: "/")
operation SayHelloXml { output: TestOutput, errors: [Error] }

structure TestOutputDocument with [TestStruct] { innerField: Nested, @required document: Document }
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
                                         id: "error_recovery_json",
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
                                             document: {}
                                             nested: { a: "" }
                                         },
                                         code: 200,
                                         body: "{\"union\": { \"A\": 5 }, \"enum\": \"A\" }"
                                     }])

apply SayHelloXml @httpResponseTests([{
                                       id: "error_recovery_xml",
                                       protocol: restXml,
                                       params: {
                                           union: { A: 5 },
                                           enum: "A",
                                           foo: "",
                                           byteValue: 0,
                                           blob: "",
                                           listValue: [],
                                           mapValue: {},
                                           doubleListValue: []
                                           nested: { a: "" }
                                       },
                                       code: 200,
                                       body: "<TestOutput><union><A>5</A></union><enum>A</enum></TestOutput>"
                                   }])
