# Rust SDK Design Tenets
> Unless you know better ones! These are our tenets today, but we'd love your thoughts. Do you wish we had different priorities? Let us know by opening and issue or starting a discussion.
1. [**Batteries included, but replaceable.**](#batteries-included-but-replaceable) The Rust SDK should provide a best-in-class experience for many use cases, **but**, but customers will use the SDK in unqiue and unexpected ways. **Meet customers where they are;** strive to be compatible with their tools. Provide mechanisms to allow customers make different choices.
2. [**Make common problems easy to solve.**](#make-common-problems-easy-to-solve) Make uncommon problems solvable. Guide customers to patterns that set them up for long-term success.
3. [**Design for the Future.**](#design-for-the-future) APIs will evolve in unpredictable directions, new protocols will gain adoption, and new services will be created that we never could have imagined. Don’t simplify or unify code today that prevents evolution tomorrow.

## Details, Justifications, and Ramifications

### Batteries included, but replaceable.

Some customers will use the Rust SDK as their first experience with async Rust, potentially **any** Rust. They may not be familiar with Tokio or the concept of an async executor. We are not afraid to have an opinion about the best solution for most customers.

Other customers will come to our SDK with specific requirements. Perhaps they’re integrating the SDK into a much larger project that uses `async_std`. Maybe they need to set custom headers, modify the user agent, or audit every request. They should be able to use the Rust SDK without forking it to meet their needs.

### Make common problems easy to solve

If solving a common problem isn’t obvious from the API, it should be obvious from the documentation. The SDK should guide users towards the best solutions for common tasks, **first** with well named methods, **second** with documentation, and **third** with real -world usage examples. Provide misuse resistant APIs. Async Rust has the potential to introduce subtle bugs; we should do our best to help customers avoid them.

### Design for the Future

APIs evolve in unpredictable ways, and it's crucial that we can evolve the SDK without breaking existing customers. This means designing the SDK so that fundamental changes to the internals can be made without altering the external interface we surface to customers:

* Keeping the shared core as small & opaque as possible.
* Don’t leak our internal dependencies to customers
* With every design choice, consider, "Can I reverse this choice in the future?"

This may not result in DRY code, and that’s OK! Code that is auto generated has different goals and tradeoffs than code that has been written by hand.
