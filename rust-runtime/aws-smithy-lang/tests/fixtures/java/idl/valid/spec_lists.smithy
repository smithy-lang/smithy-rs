$version: "2"
namespace smithy.example

list MyList {
    member: String
}

@length(min: 3, max: 10)
list MyConstrainedList {
    @length(min: 1, max: 100)
    member: String
}
