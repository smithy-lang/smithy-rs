$version: "2"

namespace ex

// An `apply` may come before the shape it targets.
apply Forward @documentation("Applied before the definition.")

string Forward

structure Structure {
    member: String

    defaulted: String = "a"
}

// Member targets, annotation traits, and a trait applied twice with the same value.
apply Structure$member @required

apply Structure @sensitive

apply Structure @documentation("Same value.")

apply Structure @documentation("Same value.")

// Applying the same default as the syntactic sugar.
apply Structure$defaulted @default("a")

/// Documentation comments before `apply` document nothing.
apply Structure$member @tags(["before"])

// Documentation comments inside a block document nothing either. A list trait applied twice in
// one block is concatenated.
apply Structure$defaulted {
    /// Ignored.
    @tags(["first"])
    @tags(["second"])
}

// Members of inline operation input.
operation Operation {
    input := {
        id: String
    }
}

apply OperationInput$id @required

// `apply` can make a shape a trait, which other shapes then use.
structure marker {}

apply marker @trait

@marker
string Marked

// Trait values that refer to shapes are resolved in the applying file.
@trait
@idRef
string reference

apply Marked @reference(Forward)
