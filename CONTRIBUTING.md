# Contributing Guidelines

Thank you for your interest in contributing to our project. Whether it's a bug report, new feature, correction, or additional
documentation, we greatly value feedback and contributions from our community.

Please read through this document before submitting any issues or pull requests to ensure we have all the necessary
information to effectively respond to your bug report or contribution.


## Reporting Bugs/Feature Requests

We welcome you to use the GitHub issue tracker to report bugs or suggest features.

When filing an issue, please check existing open, or recently closed, issues to make sure somebody else hasn't already
reported the issue. Please try to include as much information as you can. Details like these are incredibly useful:

* A reproducible test case or series of steps
* The version of our code being used
* Any modifications you've made relevant to the bug
* Anything unusual about your environment or deployment


## Contributing via Pull Requests
Contributions via pull requests are much appreciated. Before sending us a pull request, please ensure that:

1. You are working against the latest source on the *main* branch.
2. You check existing open, and recently merged, pull requests to make sure someone else hasn't addressed the problem already.
3. **You open an issue to discuss any significant work** - we would hate for your time to be wasted. Alternatively, you
   may submit an RFC. You can learn about our RFC process [here](./design/src/rfcs/overview.md).

To send us a pull request, please:

1. Fork the repository.
2. Modify the source; please focus on the specific change you are contributing. If you also reformat all the code, it will be hard for us to focus on your change.
3. Ensure local tests pass.
4. Commit to your fork using clear commit messages.
5. Send us a pull request, answering any default questions in the pull request interface. To create a changelog entry Markdown file in the `.changelog` directory, you can do either of the following:
   - Use the `new` subcommand of [changelogger](https://github.com/smithy-lang/smithy-rs/tree/main/tools/ci-build/changelogger) CLI (**preferred**)
   - Create one manually. Name the file `XXX.md`, where `XXX` can be any name you choose, as long as it is unique in the `.changelog` directory. Ensure the contents follow Markdown syntax with the YAML front matter. For reference, see [.example](https://github.com/smithy-lang/smithy-rs/blob/3e250cf9f61ee17ccd66e16314d4e47f3dd95e25/.changelog/.example) or other Markdown files in the `.changelog` directory.
6. Include updated Cargo lockfiles in your pull request if you have modified the `aws-config` crate or any crate(s) (excluding `inlineable`) under  the [rust-runtime](https://github.com/smithy-lang/smithy-rs/tree/main/rust-runtime) or the [aws/rust-runtime](https://github.com/smithy-lang/smithy-rs/tree/main/aws/rust-runtime) workspaces.
   - If you encounter merge conflicts in a lockfile when merging the latest main branch into your branch, resolve the conflicts by running  `git checkout --theirs <path to lockfile>` to keep the changes from the main branch and rerun the tests for the updated crate.
7. Pay attention to any automated CI failures reported in the pull request, and stay involved in the conversation.
8. Ask maintainers to manually trigger [canary](https://github.com/smithy-lang/smithy-rs/actions/workflows/manual-canary.yml) and [PR bot](https://github.com/smithy-lang/smithy-rs/actions/workflows/manual-pull-request-bot.yml) workflows using your pull request number. Those workflows cannot run in your fork and are always skipped as such.

GitHub provides additional document on [forking a repository](https://help.github.com/articles/fork-a-repo/) and
[creating a pull request](https://help.github.com/articles/creating-a-pull-request/).


## Finding contributions to work on
Looking at the existing issues is a great way to find something to contribute on. As our projects, by default, use the default GitHub issue labels (enhancement/bug/duplicate/help wanted/invalid/question/wontfix), looking at any 'help wanted' issues is a great place to start.


## Code of Conduct
This project has adopted the [Amazon Open Source Code of Conduct](https://aws.github.io/code-of-conduct).
For more information see the [Code of Conduct FAQ](https://aws.github.io/code-of-conduct-faq) or contact
opensource-codeofconduct@amazon.com with any additional questions or comments.


## Reporting a Vulnerability

If you discover a potential security issue in this project we ask that you notify AWS/Amazon Security
via our [vulnerability reporting page](http://aws.amazon.com/security/vulnerability-reporting/) or directly via email to aws-security@amazon.com.
Please do **not** create a public GitHub issue.


## Licensing

See the [LICENSE](LICENSE) file for our project's licensing. We will ask you to confirm the licensing of your contribution.

## More helpful information for contributors

Part of our [design docs](./design/src/overview.md) are dedicated to helpful information for contributors.
[Take a look](./design/src/contributing/overview.md).
