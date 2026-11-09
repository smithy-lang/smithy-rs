# Merge queue

Pull requests to `main` merge through a GitHub [merge queue]. The queue tests each pull request on
top of the latest `main` and every pull request queued ahead of it, then merges the tested commit.
Authors no longer need to rebase onto `main` before merging.

## Merging a pull request

A pull request can join the queue once it has an approval and its required checks pass. Then do one
of the following:

- Select **Merge when ready** in the pull request's merge box.
- Run `gh pr merge <number>`. If checks are still running, this enables auto-merge, and the pull
  request joins the queue when they pass.

The queue squash-merges each pull request. Pushing a new commit to a queued pull request removes it
from the queue.

## What the queue runs

The queue runs `.github/workflows/ci-merge-queue.yml` for each merge group. It runs the same
required checks as pull request CI, which takes roughly 35 to 60 minutes. It skips the PR bot, the
canaries, and the semver hazard check. None of those are required checks, so they couldn't block a
merge, and `ci-main.yml` runs the canaries and the semver hazard check on `main` after the merge.

The semver check compares the pull request with the merge group's parent commit, which contains
`main` and every pull request ahead of it. As in pull request CI, a pull request with breaking
changes needs the `breaking-change` label.

If a merge group fails, the queue removes that pull request and rebuilds the groups behind it.
Fix the failure, or re-run it if it was a flake, and add the pull request to the queue again.

## Pull requests from forks

Pull request CI for forks can't use repository secrets, so it doesn't run the canary or the PR bot,
and the `Ask maintainer to run the PR bot and canary workflows` check fails. That check isn't
required. The queue doesn't run the canary either, so a maintainer still invokes the
[canary](https://github.com/smithy-lang/smithy-rs/actions/workflows/manual-canary.yml) manually
before adding a fork pull request to the queue, and can invoke the
[PR bot](https://github.com/smithy-lang/smithy-rs/actions/workflows/manual-pull-request-bot.yml)
for the codegen diff.

The queue runs fork code with repository secrets, such as the ECR credentials for the build image,
so review
every commit in a fork pull request before adding it to the queue. The ruleset requires approval of
the most recent push for this reason.

## Urgent merges

For a time-sensitive change, such as a security fix, a maintainer has two options.

1. **Jump to the top of the queue.** Select **Jump the queue** when adding the pull request to the
   queue. The pull request is still tested, but every group already in progress is rebuilt behind it.
2. **Bypass the queue.** Repository admins can merge a pull request directly, without waiting for the
   queue or the required checks. In the merge box, select **Merge without waiting for requirements to
   be met (bypass branch protections)**, or run:

   ```shell
   gh pr merge <number> --admin --squash
   ```

   Nothing tests the merged result before it lands on `main`, so use this only when waiting for the
   queue isn't acceptable. Get an approval first unless the fix can't wait for one, and watch the
   `ci-main.yml` run on `main` after the merge. Bypassing doesn't allow pushing to `main` directly.

## Repository settings

The queue is configured in the `main-branch-protection` ruleset, which only repository admins can
change:

| Setting                                        | Value                                |
|------------------------------------------------|--------------------------------------|
| Merge method                                   | Squash                               |
| Build concurrency                              | 5                                    |
| Group size                                     | 1 to 5, waiting up to 5 minutes      |
| Grouping strategy                              | Only merge non-failing pull requests |
| Status check timeout                           | 180 minutes                          |
| Require branches to be up to date              | Off, because the queue does this     |
| Require approval of the most recent push       | On                                   |
| Bypass                                         | Repository admins, pull requests only |

Every required status check must be reported on merge groups, or the queue waits for it until the
status check timeout. When adding a required check, add its job to `ci-merge-queue.yml` or to
`ci.yml`, which `ci-merge-queue.yml` calls.

[merge queue]: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue
