RFC: Providing fallback credentials on external timeout
=======================================================
> Status: Implemented in [smithy-rs#2246](https://github.com/awslabs/smithy-rs/pull/2246)
>
> Applies to: client

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC proposes a fallback mechanism for credentials providers on external timeout (see the [Terminology](#terminology) section), allowing them to continue serving (possibly expired) credentials for the sake of overall reliability of the intended service; The IMDS credentials provider is an example that must fulfill such a requirement to support static stability.

Terminology
-----------

- External timeout: The name of the timeout that occurs when a duration elapses before an async call to `provide_credentials` returns. In this case, `provide_credentials` returns no credentials.
- Internal timeout: The name of the timeout that occurs when a duration elapses before an async call to some function, inside the implementation of `provide_credentials`, returns. Examples include connection timeouts, TLS negotiation timeouts, and HTTP request timeouts. Implementations of `provide_credentials` may handle these failures at their own discretion e.g. by returning _(possibly expired)_ credentials or a `CredentialsError`.
- Static stability: Continued availability of a service in the face of impaired dependencies.

Assumption
----------

This RFC is concerned only with external timeouts, as the cost of poor API design is much higher in this case than for internal timeouts. The former will affect a public trait implemented by all credentials providers whereas the latter can be handled locally by individual credentials providers without affecting one another.

Problem
-------

We have mentioned static stability. Supporting it calls for the following functional requirement, among others:
- REQ 1: Once a credentials provider has served credentials, it should continue serving them in the event of a timeout (whether internal or external) while obtaining refreshed credentials.

Today, we have the following trait method to obtain credentials:
```rust,ignore
fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
where
    Self: 'a,
```
This method returns a future, which can be raced against a timeout future as demonstrated by the following code snippet from `LazyCredentialsCache`:

```rust,ignore
let timeout_future = self.sleeper.sleep(self.load_timeout); // by default self.load_timeout is 5 seconds.
// --snip--
let future = Timeout::new(provider.provide_credentials(), timeout_future);
let result = cache
   .get_or_load(|| async move {
        let credentials = future.await.map_err(|_err| {
            CredentialsError::provider_timed_out(load_timeout)
        })??;
        // --snip--
    }).await;
// --snip--
```
This creates an external timeout for `provide_credentials`. If `timeout_future` wins the race, a future for `provide_credentials` gets dropped, `timeout_future` returns an error, and the error is mapped to `CredentialsError::ProviderTimedOut` and returned. This makes it impossible for the variable `provider` above to serve credentials as stated in REQ 1.

A more complex use case involves `CredentialsProviderChain`. It is a manifestation of the chain of responsibility pattern and keeps calling the `provide_credentials` method on each credentials provider down the chain until credentials are returned by one of them. In addition to REQ 1, we have the following functional requirement with respect to `CredentialsProviderChain`:
- REQ 2: Once a credentials provider in the chain has returned credentials, it should continue serving them even in the event of a timeout (whether internal or external) without falling back to another credentials provider.

Referring back to the code snippet above, we analyze two relevant cases (and suppose provider 2 below must meet REQ 1 and REQ 2 in each case):

**Case 1:** Provider 2 successfully loaded credentials but later failed to do so because an external timeout kicked in.

<p align="center">
<img width="750" alt="chain-provider-ext-timeout-1" src="https://user-images.githubusercontent.com/15333866/212421638-d08e4821-2dbe-497f-82c5-c78aab8acbe9.png">
</p>

The figure above illustrates an example. This `CredentialsProviderChain` consists of three credentials providers. When `CredentialsProviderChain::provide_credentials` is called, provider 1's `provide_credentials` is called but does not find credentials so passes the torch to provider 2, which in turn successfully loads credentials and returns them. The next time the method is called, provider 1 does not find credentials but neither does provider 2 this time, because an external timeout by `timeout_future` given to the whole chain kicked in and the future is dropped while provider 2's `provide_credentials` was running. Given the functional requirements, provider 2 should return the previously available credentials but today the code snippet from `LazyCredentialsCache` returns a `CredentialsError::ProviderTimedOut` instead.

**Case 2:** Provider 2 successfully loaded credentials but later was not reached because its preceding provider was still running when an external timeout kicked in.

<p align="center">
<img width="750" alt="chain-provider-ext-timeout-2" src="https://user-images.githubusercontent.com/15333866/212421712-8c6eab11-a0c1-4229-8ba3-67b0bb6056e7.png">
</p>

The figure above illustrates an example with the same setting as the previous figure. Again, when `CredentialsProviderChain::provide_credentials` is called the first time, provider 1 does not find credentials but provider 2 does. The next time the method is called, provider 1 is still executing `provide_credentials` and then an external timeout by `timeout_future` kicked in. Consequently, the execution of `CredentialsProviderChain::provide_credentials` has been terminated. Given the functional requirements, provider 2 should return the previously available credentials but today the code snippet from `LazyCredentialsCache` returns `CredentialsError::ProviderTimedOut` instead.


Proposal
--------
To address the problem in the previous section, we propose to add a new method to the `ProvideCredentials` trait called `fallback_on_interrupt`. This method allows credentials providers to have a fallback mechanism on an external timeout and to serve credentials to users if needed. There are two options as to how it is implemented, either as a synchronous primitive or as an asynchronous primitive.

#### Option A: Synchronous primitive
```rust,ignore
pub trait ProvideCredentials: Send + Sync + std::fmt::Debug {
    // --snip--

    fn fallback_on_interrupt(&self) -> Option<Credentials> {
        None
    }
}
```
- :+1: Users can be guided to use only synchronous primitives when implementing `fallback_on_interrupt`.
- :-1: It cannot support cases where fallback credentials are asynchronously retrieved.
- :-1: It may turn into a blocking operation if it takes longer than it should.

#### Option B: Asynchronous primitive
```rust,ignore
mod future {
    // --snip--

    // This cannot use `OnlyReady` in place of `BoxFuture` because
    // when a chain of credentials providers implements its own
    // `fallback_on_interrupt`, it needs to await fallback credentials
    // in its inner providers. Thus, `BoxFuture` is required.
    pub struct FallbackOnInterrupt<'a>(NowOrLater<Option<Credentials>, BoxFuture<'a, Option<Credentials>>>);

    // impls for FallbackOnInterrupt similar to those for the ProvideCredentials future newtype
}

pub trait ProvideCredentials: Send + Sync + std::fmt::Debug {
    // --snip--

    fn fallback_on_interrupt<'a>(&'a self) -> future::FallbackOnInterrupt<'a> {
        future::FallbackOnInterrupt::ready(None)
    }
}
```
- :+1: It is async from the beginning, so less likely to introduce a breaking change.
- :-1: We may have to consider yet another timeout for `fallback_on_interrupt` itself.

Option A cannot be reversible in the future if we are to support the use case for asynchronously retrieving the fallback credentials, whereas option B allows us to continue supporting both ready and pending futures when retrieving the fallback credentials. However, `fallback_on_interrupt` is supposed to return credentials that have been set aside in case `provide_credentials` is timed out. To express that intent, we choose option A and document that users should NOT go fetch new credentials in `fallback_on_interrupt`.

The user experience for the code snippet in question will look like this once this proposal is implemented:
```rust,ignore
let timeout_future = self.sleeper.sleep(self.load_timeout); // by default self.load_timeout is 5 seconds.
// --snip--
let future = Timeout::new(provider.provide_credentials(), timeout_future);
let result = cache
    .get_or_load(|| {
        async move {
           let credentials = match future.await {
                Ok(creds) => creds?,
                Err(_err) => match provider.fallback_on_interrupt() { // can provide fallback credentials
                    Some(creds) => creds,
                    None => return Err(CredentialsError::provider_timed_out(load_timeout)),
                }
            };
            // --snip--
        }
    }).await;
// --snip--
```


How to actually implement this RFC
----------------------------------

Almost all credentials providers do not have to implement their own `fallback_on_interrupt` except for `CredentialsProviderChain` (`ImdsCredentialsProvider` also needs to implement `fallback_on_interrupt` when we are adding static stability support to it but that is outside the scope of this RFC).

Considering the two cases we analyzed above, implementing `CredentialsProviderChain::fallback_on_interrupt` is not so straightforward. Keeping track of whose turn in the chain it is to call `provide_credentials` when an external timeout has occurred is a challenging task. Even if we figured it out, that would still not satisfy `Case 2` above, because it was provider 1 that was actively running when the external timeout kicked in, but the chain should return credentials from provider 2, not from provider 1.

With that in mind, consider instead the following approach:
```rust,ignore
impl ProvideCredentials for CredentialsProviderChain {
    // --snip--

    fn fallback_on_interrupt(&self) -> Option<Credentials> { {
        for (_, provider) in &self.providers {
            match provider.fallback_on_interrupt() {
                creds @ Some(_) => return creds,
                None => {}
            }
        }
        None
    }
}
```
`CredentialsProviderChain::fallback_on_interrupt` will invoke each provider's `fallback_on_interrupt` method until credentials are returned by one of them. It ensures that the updated code snippet for `LazyCredentialsCache` can return credentials from provider 2 in both `Case 1` and `Case 2`. Even if `timeout_future` wins the race, the execution subsequently calls `provider.fallback_on_interrupt()` to obtain fallback credentials from provider 2, assuming provider 2's `fallback_on_interrupt` is implemented to return fallback credentials accordingly.

The downside of this simple approach is that the behavior is not clear if more than one credentials provider in the chain can return credentials from their `fallback_on_interrupt`. Note, however, that it is the exception rather than the norm for a provider's `fallback_on_interrupt` to return fallback credentials, at least at the time of writing (01/13/2023). The fact that it returns fallback credentials means that the provider successfully loaded credentials at least once, and it usually continues serving credentials on subsequent calls to `provide_credentials`.

Should we have more than one provider in the chain that can potentially return fallback credentials from `fallback_on_interrupt`, we could configure the behavior of `CredentialsProviderChain` managing in what order and how each `fallback_on_interrupt` should be executed. See the [Possible enhancement
](#possible-enhancement) section for more details. The use case described there is an extreme edge case, but it's worth exploring what options are available to us with the proposed design.

Alternative
-----------

In this section, we will describe an alternative approach that we ended up dismissing as unworkable.

Instead of `fallback_on_interrupt`, we considered the following method to be added to the `ProvideCredentials` trait:
```rust,ignore
pub trait ProvideCredentials: Send + Sync + std::fmt::Debug {
    // --snip--

    /// Returns a future that provides credentials within the given `timeout`.
    ///
    /// The default implementation races `provide_credentials` against
    /// a timeout future created from `timeout`.
    fn provide_credentials_with_timeout<'a>(
        &'a self,
        sleeper: Arc<dyn AsyncSleep>,
        timeout: Duration,
    ) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        let timeout_future = sleeper.sleep(timeout);
        let future = Timeout::new(self.provide_credentials(), timeout_future);
        future::ProvideCredentials::new(async move {
            let credentials = future
                .await
                .map_err(|_err| CredentialsError::provider_timed_out(timeout))?;
            credentials
        })
    }
```
`provide_credentials_with_timeout` encapsulated the timeout race and allowed users to specify how long the external timeout for `provide_credentials` would be. The code snippet from `LazyCredentialsCache` then looked like
```rust,ignore
let sleeper = Arc::clone(&self.sleeper);
let load_timeout = self.load_timeout; // by default self.load_timeout is 5 seconds.
// --snip--
let result = cache
    .get_or_load(|| {
        async move {
            let credentials = provider
                .provide_credentials_with_timeout(sleeper, load_timeout)
                .await?;
            // --snip--
        }
    }).await;
// --snip--
```
However, implementing `CredentialsProviderChain::provide_credentials_with_timeout` quickly ran into the following problem:
```rust,ignore
impl ProvideCredentials for CredentialsProviderChain {
    // --snip--

    fn provide_credentials_with_timeout<'a>(
        &'a self,
        sleeper: Arc<dyn AsyncSleep>,
        timeout: Duration,
    ) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials_with_timeout(sleeper, timeout))
    }
}

impl CredentialsProviderChain {
    // --snip--

    async fn credentials_with_timeout(
        &self,
        sleeper: Arc<dyn AsyncSleep>,
        timeout: Duration,
    ) -> provider::Result {
        for (_, provider) in &self.providers {
            match provider
                .provide_credentials_with_timeout(Arc::clone(&sleeper), /* how do we calculate timeout for each provider ? */)
                .await
            {
                Ok(credentials) => {
                    return Ok(credentials);
                }
                Err(CredentialsError::ProviderTimedOut(_)) => {
                    // --snip--
                }
                Err(err) => {
                   // --snip--
                }
           }
        }
        Err(CredentialsError::provider_timed_out(timeout))
    }
```
There are mainly two problems with this approach. The first problem is that as shown above, there is no sensible way to calculate a timeout for each provider in the chain. The second problem is that exposing a parameter like `timeout` at a public trait's level is giving too much control to users; delegating overall timeout to the individual provider means each provider has to get it right.

Changes checklist
-----------------

- [ ] Add `fallback_on_interrupt` method to the `ProvideCredentials` trait with the default implementation
- [ ] Implement `CredentialsProviderChain::fallback_on_interrupt`
- [ ] Implement `DefaultCredentialsChain::fallback_on_interrupt`
- [ ] Add unit tests for `Case 1` and `Case 2`

Possible enhancement
--------------------
We will describe how to customize the behavior for `CredentialsProviderChain::fallback_on_interrupt`. We are only demonstrating how much the proposed design can be extended and currently do not have concrete use cases to implement using what we present in this section.

As described in the [Proposal](#proposal) section, `CredentialsProviderChain::fallback_on_interrupt` traverses the chain from the head to the tail and returns the first fallback credentials found. This precedence policy works most of the time, but when we have more than one provider in the chain that can potentially return fallback credentials, it could break in the following edge case (we are still basing our discussion on the code snippet from `LazyCredentialsCache` but forget REQ 1 and REQ 2 for the sake of simplicity).

<p align="center">
<img width="800" alt="fallback_on_interrupt_appendix excalidraw" src="https://user-images.githubusercontent.com/15333866/213618808-d19892d8-5c83-4039-9940-280dcd2a8cf1.png">
</p>

During the first call to `CredentialsProviderChain::provide_credentials`, provider 1 fails to load credentials, maybe due to an internal timeout, and then provider 2 succeeds in loading its credentials (call them credentials 2) and internally stores them for `Provider2::fallback_on_interrupt` to return them subsequently. During the second call, provider 1 succeeds in loading credentials (call them credentials 1) and internally stores them for `Provider1::fallback_on_interrupt` to return them subsequently. Suppose, however, that credentials 1's expiry is earlier than credentials 2's expiry. Finally, during the third call, `CredentialsProviderChain::provide_credentials` did not complete due to an external timeout. `CredentialsProviderChain::fallback_on_interrupt` then returns credentials 1, when it should return credentials 2 whose expiry is later, because of the precedence policy.

This a case where `CredentialsProviderChain::fallback_on_interrupt` requires the recency policy for fallback credentials found in provider 1 and provider 2, not the precedence policy. The following figure shows how we can set up such a chain:

<p align="center">
<img width="700" alt="heterogeneous_policies_for_fallback_on_interrupt" src="https://user-images.githubusercontent.com/15333866/213755875-ac6fddbc-0f1b-4437-af16-6e0dbe08ae04.png">
</p>

The outermost chain is a `CredentialsProviderChain` and follows the precedence policy for `fallback_on_interrupt`. It contains a sub-chain that, in turn, contains provider 1 and provider 2. This sub-chain implements its own `fallback_on_interrupt` to realize the recency policy for fallback credentials found in provider 1 and provider 2. Conceptually, we have
```rust,ignore
pub struct FallbackRecencyChain {
    provider_chain: CredentialsProviderChain,
}

impl ProvideCredentials for FallbackRecencyChain {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        // Can follow the precedence policy for loading credentials
        // if it chooses to do so.
    }

    fn fallback_on_interrupt(&self) -> Option<Credentials> {
        // Iterate over `self.provider_chain` and return
        // fallback credentials whose expiry is the most recent.
    }
}
```
We can then compose the entire chain like so:
```rust,ignore
let provider_1 = /* ... */
let provider_2 = /* ... */
let provider_3 = /* ... */

let sub_chain = CredentialsProviderChain::first_try("Provider1", provider_1)
    .or_else("Provider2", provider_2);

let recency_chain = /* Create a FallbackRecencyChain with sub_chain */

let final_chain = CredentialsProviderChain::first_try("fallback_recency", recency_chain)
    .or_else("Provider3", provider_3);
```
The `fallback_on_interrupt` method on `final_chain` still traverses from the head to the tail, but once it hits `recency_chain`, `fallback_on_interrupt` on `recency_chain` respects the expiry of fallback credentials found in its inner providers.

What we have presented in this section can be generalized thanks to chain composability. We could have different sub-chains, each implementing its own policy for `fallback_on_interrupt`.
