# Rust SDK Design Tenets
> Unless you know better ones! These are our tenets today, but we'd love your thoughts! Do you wish we had different priorities? Let us know by opening and issue or starting a discussion.
1. [**Batteries included, but replaceable.**](#batteries-included-but-replaceable) The Rust SDK should provide a best-in-class experience for many use cases, **but**, we can’t foresee all the situations the customer will use our software. **Meet customers where they are;** strive to be compatible with their tools. Provide escape hatches to allow customers make different choices.
2. [**Make common problems easy to solve.**](#make-common-problems-easy-to-solve) Make uncommon problems solvable. Lead customers into [the pit of success](https://blog.codinghorror.com/falling-into-the-pit-of-success/).
3. [**Design for the Future.**](#design-for-the-future) We can’t know how APIs will evolve, what protocols will gain adoption, and what new service will be created. Don’t simplify or unify code today that prevents evolution tomorrow.

## Details, Justification, and Ramifications

### Batteries included, but replaceable.

Some customers will use the Rust SDK as their first experience with async Rust, potentially **any** Rust. They aren't familiar with Tokio or the concept of an async executor. They don’t know that the ecosystem is fragmented. We shouldn't be afraid to have an opinion about the best solution for most customers.

Other customers will come to our SDK with specific requirements. Perhaps they’re integrating the SDK into a much larger project that uses `async_std`. Perhaps they need to set custom headers, modify the user agent, or audit every request. They should be able to use the Rust SDK without forking it to get what they need.

### Make common problems easy to solve

If solving a common problem isn’t obvious from the API, it should be obvious from the documentation. The SDK should guide users towards the best solutions for common tasks, **first** with well named methods, **second** with documentation, and **third** with real -world usage examples. Don't provide APIs that are easy to misuse. Async Rust exposes a [number of footguns](https://github.com/rusoto/rusoto/issues/1726); we should do our best to help customers avoid them.

### **Design for the Future**

We can't consider every potential future API evolution, so it’s crucial that we can evolve the SDK without breaking existing clients. This means limiting the blast radius of changes:

* Keeping the shared core as small & opaque as possible.
* Don’t leak our internal dependencies to customers

This may not result in DRY code, and that’s OK! Code that is auto generated has different goals and tradeoffs than code that has been written by hand.
