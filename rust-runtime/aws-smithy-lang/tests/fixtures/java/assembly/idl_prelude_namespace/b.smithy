$version: "2"

namespace ex

structure Uses {
    @length(min: 1)
    name: String

    foo: smithy.api#Foo
}
