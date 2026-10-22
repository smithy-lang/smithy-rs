$version: "2"
namespace smithy.example

map IntegerMap {
    key: String,
    value: Integer
}

@length(min: 0, max: 100)
map ConstrainedIntegerMap {
    @length(min: 1, max: 10)
    key: String,

    @range(min: 1, max: 1000)
    value: Integer
}
