$version: "2"
namespace smithy.example

enum Suit {
    @deprecated
    @enumValue("diamond")
    DIAMOND

    @enumValue("club")
    CLUB

    @enumValue("heart")
    HEART

    @enumValue("spade")
    SPADE
}
