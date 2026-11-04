$version: "2"
namespace smithy.example

string MyString

apply MyString {
    @documentation("This is my string!")
    @length(min: 1, max: 10)
}
