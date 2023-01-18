RFC: Providing fallback credentials on external timeout
=======================================================

> Status: RFC
>
> Applies to: client

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC proposes a fallback mechanism for credentials providers on external timeout (see the [Terminology](#terminology) section), allowing them to continue serving (possibly stale) credentials for the sake of overall reliability of the intended service; The IMDS credentials provider is an example that must fulfill such a requirement to support static stability.

Terminology
-----------

- External timeout: The name of the timeout that occurs when a duration elapses before an async call to `provide_credentials` returns. In this case, `provide_credentials` returns no credentials.
- Internal timeout: The name of the timeout that occurs when a duration elapses before an async call to some function, inside the implementation of `provide_credentials`, returns. Examples include connection timeouts, TLS negotiation timeouts, and HTTP request timeouts. Implementations of `provide_credentials` may handle these failures at their own discretion e.g. by returning _(possibly expired)_ credentials or a `CredentialsError`.

Assumption
----------

This RFC is concerned only with external timeouts, as the cost of poor API design is much higher than in this case than for internal timeouts. The former will affect a public trait implemented by all credentials providers whereas the latter can be handled locally by individual credentials providers without affecting one another.

Problem
-------

We have mentioned static stability. Supporting it calls for the following functional requirement, among others:
- REQ 1: Once a credentials provider has served credentials, it should continue serving them in the event of a timeout (whether internal or external) while obtaining refreshed credentials.

Today, we have the following trait method to obtain credentials:
```rust
fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
where
    Self: 'a,
```
This method returns a future, which can be raced against a timeout future as demonstrated by the following code snippet from `LazyCredentialsCache`:

```rust
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

A slightly more complex use case involves `CredentialsProviderChain`. It is a manifestation of the chain of responsibility pattern and keeps calling the `provide_credentials` method on each credentials provider down the chain until credentials are returned by one of them. In addition to REQ 1, we have the following functional requirement with respect to `CredentialsProviderChain`:
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
To address the problem in the previous section, we propose to add a new method to the `ProvideCredentials` trait called `on_timeout` that looks something like this:
```rust
mod future {
    // --snip--

    pub struct OnTimeout<'a>(NowOrLater<Option<Credentials>, BoxFuture<'a, Option<Credentials>>>);

    // impls for OnTimeout similar to those for the ProvideCredentials future newtype
}

pub trait ProvideCredentials: Send + Sync + std::fmt::Debug {
    // --snip--

    fn on_timeout<'a>(&'a self) -> future::OnTimeout<'a> {
        future::OnTimeout::ready(None)
    }
}
```
This method allows credentials providers to have a fallback mechanism on an external timeout and to serve credentials to users if needed. It comes with a default implementation to return a future that immediately yields `None`. Credentials providers that need a different behavior can choose to override the method.

The user experience for the code snippet in question will look like this once this proposal is implemented:
```rust
let timeout_future = self.sleeper.sleep(self.load_timeout); // by default self.load_timeout is 5 seconds.
// --snip--
let future = Timeout::new(provider.provide_credentials(), timeout_future);
let result = cache
    .get_or_load(|| {
        async move {
           let credentials = match future.await {
                Ok(creds) => creds?,
                Err(_err) => match provider.on_timeout().await { // can provide fallback credentials
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

Almost all credentials providers do not have to implement their own `on_timeout` except for `CredentialsProviderChain` (`ImdsCredentialsProvider` also needs to implement `on_timeout` when we are adding static stability support to it but that is outside the scope of this RFC).

Considering the two cases we analyzed above, implementing `CredentialsProviderChain::on_timeout` is not so straightforward. Keeping track of whose turn in the chain it is to call `provide_credentials` when an external timeout has occurred is a challenging task. Even if we figured it out, that would still not satisfy `Case 2` above, because it was provider 1 that was actively running when the external timeout kicked in, but the chain should return credentials from provider 2, not from provider 1.

With that in mind, consider instead the following approach:
```rust
impl ProvideCredentials for CredentialsProviderChain {
    // --snip--

    fn on_timeout<'a>(&'a self) -> future::OnTimeout<'a> {
        future::OnTimeout::new(async move {
            for (_, provider) in &self.providers {
                match provider.on_timeout().await {
                    creds @ Some(_) => return creds,
                    None => {}
                }
            }
            None
        })
    }
}
```
`CredentialsProviderChain::on_timeout` will invoke each provider's `on_timeout` method until credentials are returned by one of them. It ensures that the updated code snippet for `LazyCredentialsCache` can return credentials from provider 2 in both `Case 1` and `Case 2`. Even if `timeout_future` wins the race, the execution subsequently calls `provider.on_timeout().await` to obtain fallback credentials from provider 2, assuming provider 2's `on_timeout` is implemented to return credentials retrieved on the first call to `CredentialsProviderChain::provide_credentials`.

The downside of this simple approach is that the behavior is not clear if more than one credentials provider in the chain can return credentials from their `on_timeout`. Note, however, that it is the exception rather than the norm for a provider's `on_timeout` to return fallback credentials, at least at the time of writing (01/13/2023). The fact that it returns fallback credentials means that the provider successfully loaded credentials at least once, and it usually continues serving credentials on subsequent calls to `provide_credentials`.

Should we have more than one provider in the chain that can potentially return fallback credentials from `on_timeout`, we could configure the behavior of `CredentialsProviderChain` managing in what order and how each `on_timeout` should be executed; it can be possible to introduce a new API without breaking changes. That being said, we first need to investigate and understand the use case behind it to design the right API.

Alternative
-----------

In this section, we will describe an alternative approach that we ended up dismissing as unworkable.

Instead of `on_timeout`, we considered the following method to be added to the `ProvideCredentials` trait:
```rust
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
```rust
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
```rust
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

- [ ] Add `on_timeout` method to the `ProvideCredentials` trait with the default implementation
- [ ] Implement the `OnTimeout` future newtype
- [ ] Implement `CredentialsProviderChain::on_timeout`
- [ ] Add unit tests for `Case 1` and `Case 2`
