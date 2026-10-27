$version: "2"
namespace ex

structure S {a: String, b: Integer,}
@length(min: 1, max: 2,) string T
list L {member: String}
map M {key: String, value: String}
enum E {A, C,
    B = "b",
}
