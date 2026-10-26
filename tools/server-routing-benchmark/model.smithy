$version: "2.0"
namespace benchmark.routing

// PROTOCOLS
service BenchmarkService {
    version: "1.0"
    operations: [Echo, Ping, Read, Write, List, Find, Update, Delete]
}

@http(method: "POST", uri: "/echo", code: 200)
operation Echo { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/ping", code: 200)
operation Ping { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/read", code: 200)
operation Read { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/write", code: 200)
operation Write { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/list", code: 200)
operation List { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/find", code: 200)
operation Find { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/update", code: 200)
operation Update { input: EchoInput, output: EchoOutput }
@http(method: "POST", uri: "/delete", code: 200)
operation Delete { input: EchoInput, output: EchoOutput }

structure EchoInput { value: String }
structure EchoOutput { @required value: String }
