<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Collection Defaults
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: Implemented
>
> Applies to: client

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC proposes a **breaking change** to how generated clients automatically provide default values for collections. Currently the SDK generated fields for `List` generate optional values:
```rust,ignore
    /// <p> Container for elements related to a particular part.
    pub fn parts(&self) -> Option<&[crate::types::Part]> {
        self.parts.as_deref()
    }
```
This is _almost never_ what users want and leads to code noise when using collections:
```rust,ignore
async fn get_builds() {
    let project = codebuild
        .list_builds_for_project()
        .project_name(build_project)
        .send()
        .await?;
    let build_ids = project
        .ids()
        .unwrap_or_default();
    //  ^^^^^^^^^^^^^^^^^^ this is pure noise
}
```

This RFC proposes unwrapping into default values in our accessor methods.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **Accessor**: The Rust SDK defines accessor methods on modeled structures for fields to make them more convenient for users
- **Struct field**: The accessors point to concrete fields on the struct itself.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

In the current version of the SDK, users must call `.unwrap_or_default()` frequently.
Once this RFC is implemented, users will be able to use these accessors directly. In the rare case where users need to
distinguish be `None` and `[]`, we will direct users towards `model.<field>.is_some()`.

```rust,ignore
async fn get_builds() {
    let project = codebuild
        .list_builds_for_project()
        .project_name(build_project)
        .send()
        .await?;
    let build_ids = project.ids();
    // Goodbye to this line:
    //    .unwrap_or_default();
}
```

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

In order to implement this feature, we need update the code generate accessors for lists and maps to add `.unwrap_or_default()`. Because we are returning slices `unwrap_or_default()` does not produce any additional allocations for empty collection.

### Could this be implemented for `HashMap`?
This works for lists because we are returning a slice (allowing a statically owned `&[]` to be returned.) If we want to support HashMaps in the future this _is_ possible by using `OnceCell` to create empty HashMaps for requisite types. This would allow us to return references to those empty maps.

### Isn't this handled by the default trait?
No, many existing APIs don't have the default trait.
<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------
Estimated total work: 2 days
- [x] Update accessor method generation to auto flatten lists
- [x] Update docs for accessors to guide users to `.field.is_some()` if they MUST determine if the field was set.
