# Recursive Shapes
> Note: Throughout this document, the word "box" always refers to a Rust [`Box<T>`](https://doc.rust-lang.org/std/boxed/struct.Box.html), a heap allocated pointer to T, and not the Smithy concept of boxed vs. unboxed.

Recursive shapes pose a problem for Rust, because the following Rust code will not compile:

```rust,compile_fail
struct TopStructure {
    intermediate: IntermediateStructure
}

struct IntermediateStructure {
    top: Option<TopStructure>
}
```

```text
  |
3 | struct TopStructure {
  | ^^^^^^^^^^^^^^^^^^^ recursive type has infinite size
4 |     intermediate: IntermediateStructure
  |     ----------------------------------- recursive without indirection
  |
  = help: insert indirection (e.g., a `Box`, `Rc`, or `&`) at some point to make `main::TopStructure` representable
```

This occurs because Rust types must be a size known at compile time. The way around this, as the message suggests, is to Box the offending type. `smithy-rs` implements this design in [RecursiveShapeBoxer.kt](https://github.com/smithy-lang/smithy-rs/blob/main/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/transformers/RecursiveShapeBoxer.kt)

To support this, as the message suggests, we must "`Box`" the offending type. There is a touch of trickiness—only one element in the cycle needs to be boxed, but we need to select it deterministically such that we always pick the same element between multiple codegen runs. To do this the Rust SDK will:

1. Topologically sort the graph of shapes.
2. Identify cycles that do not pass through an existing Box<T>, List, Set, or Map
3. For each cycle, select the earliest shape alphabetically & mark it as Box<T> in the Smithy model by attaching the custom `RustBoxTrait` to the member.
4. Go back to step 1.

This would produce valid Rust:

```rust
struct TopStructure {
    intermediate: IntermediateStructure
}

struct IntermediateStructure {
    top: Box<Option<TopStructure>>
}
```

**Backwards Compatibility Note!**

Box<T> is not generally compatible with T in Rust. There are several unlikely but valid model changes that will cause the SDK to produce code that may break customers. If these are problematic, all are avoidable with customizations.

1. A recursive link is added to an existing structure. This causes a member that was not boxed before to become Box<T>.

    > **Workaround**: Mark the new member as Box<T> in a customization.

1. A field is removed from a structure that removes the recursive dependency. The SDK would generate T instead of Box<T>.

    > **Workaround**: Mark the member that used to be boxed as Box<T> in a customization. The Box will be unnecessary, but we will keep it for backwards compatibility.
