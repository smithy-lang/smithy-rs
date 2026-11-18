$version: "2"
metadata foo = {
    hello: 123,
    "foo": "456",
    testing: """
        Hello!
        """,
    an_array: [10.5],
    nested_object: {
        hello_there: true
    }, // <-- Trailing comma
}
metadata arrays = [[], [true], [1, "hello",]]
metadata objects = [{}, {foo: true}, {foo: "hello", "bar": [1, 2, {}]}]
metadata numbers = [0, 0.0, 1234, -1234.1234, 1e+2, 1.0e-10]
metadata keywords = [true, false, null]
