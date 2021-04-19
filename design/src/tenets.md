# Rust SDK Design Tenets
1. **Batteries included, but replaceable.** The Rust SDK should provide a best-in-class experience for many use cases, **but**, we can’t foresee all the situations the customer will use our software. **Meet customers where they are;** strive to be compatible with their tools. Provide escape hatches to allow customers make different choices.
2. **Dependencies must be a force multiplier for customers.** Countless previous SDKs demonstrated risks of taking on third party dependencies. Using someone else’s code is high leverage for the SDK developers, but to be included, a dependency must also be high leverage for the customers.
3. **Customer experience > our experience.** Our experience developing the SDK is important, but we have limited resources and need to prioritize what makes the best possible SDK for the customer. Although our productivity is important, we must only prioritize ourselves over the customer deliberately and carefully.
4. **Make common problems easy to solve.** Make uncommon problems solvable. Lead customers into [the pit of success](https://blog.codinghorror.com/falling-into-the-pit-of-success/).
5. **Design for the Future.** We can’t know how APIs will evolve, what protocols will gain adoption, and what new service will be created. Don’t simplify or unify code today that prevents evolution tomorrow.

## Details, Justification, and Ramifications

### Batteries included, but replaceable.

Some customers will use the Rust SDK as their first experience with async Rust, potentially **any** Rust. They aren't familiar with Tokio or the concept of an async executor. They don’t know that the ecosystem is fragmented. We shouldn't be afraid to have an opinion about the best solution for most customers.

Other customers will come to our SDK with specific requirements. Perhaps they’re integrating the SDK into a much larger project that uses `async_std`. Perhaps they need to set custom headers, modify the user agent, or audit every request. They should be able to use the Rust SDK without forking it to get what they need.

### **Customer experience > our experience**

The choices that make the SDK easy to build may clash with decisions that make the best SDK for the customer. Although our productivity is important, we must only prioritize ourselves over the customer deliberately and carefully.

Although ***we*** may struggle to compile all 260 services at once, most customers will only compile one or two at a time. We cannot confuse problems that are easy to measure over problems that deliver the most customer value.

### Make common problems easy to solve

If solving a common problem isn’t obvious from the API, it should be obvious from the documentation. Don’t let customers shoot themselves in foot. Especially, with async Rust, there are a [number of footguns](https://github.com/rusoto/rusoto/issues/1726). The first priority is providing tools to solve common problems in an idiomatic way. If this isn’t possible, provide easy-to-find authoritative documentation with numerous examples following best practices.

### **Design for the Future**

We can’t consider every potential future API evolution, so it’s crucial that we can add new modes for new services without breaking existing clients. This means limiting the blast radius of changes:

* Keeping the shared core as small & opaque as possible.
* Don’t leak our internal dependencies to customers

This may fly in the face of DRY code, and that’s OK! Codegen is different than hand written code.
