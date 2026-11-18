$version: "2"

namespace ex

// Applied traits from every definition merge: arrays concatenate.
@tags(["a"])
string Merged

structure Record {
    @tags(["a"])
    member: String

    // Equal value assignments in every definition.
    same: String = "x"

    // A value assignment is replaced by a trait applied in another file.
    overridden: String = "a"
}
