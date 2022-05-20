#!/usr/bin/env python3
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# This script can be run and tested locally. To do so, you should check out
# a second smithy-rs repository so that you can work on the script and still
# run it without it immediately bailing for an unclean working tree.
#
# Example:
# `smithy-rs/` - the main repo you're working out of
# `test/smithy-rs/` - the repo you're testing against
#
# ```
# $ cd test/smithy-rs
# $ ../../smithy-rs/tools/codegen-diff-revisions.py . <some commit hash to diff against>
# ```
#
# It will diff the generated code from HEAD against any commit hash you feed it. If you want to test
# a specific range, change the HEAD of the test repository.
#
# This script requires `diff2html-cli` to be installed from NPM:
# ```
# $ npm install -g diff2html-cli@5.1.11
# ```
# Make sure the local version matches the version referenced from the GitHub Actions workflow.

import os
import sys
import subprocess
import tempfile
import shlex


HEAD_BRANCH_NAME = "__tmp-localonly-head"
BASE_BRANCH_NAME = "__tmp-localonly-base"
OUTPUT_PATH = "tmp-codegen-diff/"

COMMIT_AUTHOR_NAME = "GitHub Action (generated code preview)"
COMMIT_AUTHOR_EMAIL = "generated-code-action@github.com"

CDN_URL = "https://d2luzm2xt3nokh.cloudfront.net"


def running_in_docker_build():
    return os.environ.get("SMITHY_RS_DOCKER_BUILD_IMAGE") == "1"


def main():
    if len(sys.argv) != 3:
        eprint("Usage: codegen-diff-revisions.py <repository root> <base commit sha>")
        sys.exit(1)

    repository_root = sys.argv[1]
    base_commit_sha = sys.argv[2]
    os.chdir(repository_root)
    head_commit_sha = get_cmd_output("git rev-parse HEAD")

    # Make sure the working tree is clean
    if get_cmd_status("git diff --quiet") != 0:
        eprint("working tree is not clean. aborting")
        sys.exit(1)

    if running_in_docker_build():
        eprint(f"Fetching base revision {base_commit_sha} from GitHub...")
        run(f"git fetch --no-tags --progress --no-recurse-submodules --depth=1 origin {base_commit_sha}")

    # Generate code for HEAD
    eprint(f"Creating temporary branch with generated code for the HEAD revision {head_commit_sha}")
    run(f"git checkout {head_commit_sha} -b {HEAD_BRANCH_NAME}")
    generate_and_commit_generated_code(head_commit_sha)

    # Generate code for base
    eprint(f"Creating temporary branch with generated code for the base revision {base_commit_sha}")
    run(f"git checkout {base_commit_sha} -b {BASE_BRANCH_NAME}")
    generate_and_commit_generated_code(base_commit_sha)

    bot_message = make_diffs(base_commit_sha, head_commit_sha)
    write_to_file(f"{OUTPUT_PATH}/bot-message", bot_message)

    # Clean-up that's only really useful when testing the script in local-dev
    if not running_in_docker_build():
        run("git checkout main")
        run(f"git branch -D {BASE_BRANCH_NAME}")
        run(f"git branch -D {HEAD_BRANCH_NAME}")


def generate_and_commit_generated_code(revision_sha):
    # Clean the build artifacts before continuing
    run("rm -rf aws/sdk/build")
    run("./gradlew codegen:clean codegen-server:clean aws:sdk-codegen:clean")

    # Generate code
    run("./gradlew --rerun-tasks :aws:sdk:assemble")
    run("./gradlew --rerun-tasks :codegen-server-test:assemble")
    run("./gradlew --rerun-tasks :codegen-server-test:python:assemble")

    # Move generated code into codegen-diff/ directory
    run(f"rm -rf {OUTPUT_PATH}")
    run(f"mkdir {OUTPUT_PATH}")
    run(f"mv aws/sdk/build/aws-sdk {OUTPUT_PATH}")
    run(f"mv codegen-server-test/build/smithyprojections/codegen-server-test {OUTPUT_PATH}")
    run(f"mv codegen-server-test/python/build/smithyprojections/codegen-server-test-python {OUTPUT_PATH}")

    # Clean up the server-test folder
    run(f"rm -rf {OUTPUT_PATH}/codegen-server-test/source")
    run(f"rm -rf {OUTPUT_PATH}/codegen-server-test-python/source")
    run(f"find {OUTPUT_PATH}/codegen-server-test | "
        f"grep -E 'smithy-build-info.json|sources/manifest|model.json' | "
        f"xargs rm -f", shell=True)
    run(f"find {OUTPUT_PATH}/codegen-server-test-python | "
        f"grep -E 'smithy-build-info.json|sources/manifest|model.json' | "
        f"xargs rm -f", shell=True)

    run(f"git add -f {OUTPUT_PATH}")
    run(f"git -c 'user.name=GitHub Action (generated code preview)' "
        f"-c 'user.name={COMMIT_AUTHOR_NAME}' "
        f"-c 'user.email={COMMIT_AUTHOR_EMAIL}' "
        f"commit --no-verify -m 'Generated code for {revision_sha}' --allow-empty")


# Writes an HTML template for diff2html so that we can add contextual information
def write_html_template(title, subtitle, tmp_file):
    tmp_file.writelines(map(lambda line: line.encode(), [
        "<!doctype html>",
        "<html>",
        "<head>",
        '  <metadata charset="utf-8">',
        f'  <title>Codegen diff for the {title}: {subtitle}</title>',
        '  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/9.9.0/styles/github.min.css" / >',
        '  <!--diff2html-css-->',
        '  <!--diff2html-js-ui-->',
        '  <script>',
        '  document.addEventListener("DOMContentLoaded", () => {',
        '    const targetElement = document.getElementById("diff");',
        '    const diff2htmlUi = new Diff2HtmlUI(targetElement);',
        '    //diff2html-fileListToggle',
        '    //diff2html-synchronisedScroll',
        '    //diff2html-highlightCode',
        '  });',
        '  </script>',
        "</head>",
        "<body>",
        f"  <h1>Codegen diff for the {title}</h1>",
        f"  <p>{subtitle}</p>",
        '  <div id="diff">',
        '    <!--diff2html-diff-->',
        '  </div>',
        "</body>",
        "</html>",
    ]))
    tmp_file.flush()


def make_diff(title, path_to_diff, base_commit_sha, head_commit_sha, suffix, whitespace):
    whitespace_flag = "" if whitespace else "-b"
    diff_exists = get_cmd_status(f"git diff --quiet {whitespace_flag} "
                                 f"{BASE_BRANCH_NAME} {HEAD_BRANCH_NAME} -- {path_to_diff}")
    if diff_exists == 0:
        eprint(f"No diff output for {base_commit_sha}..{head_commit_sha}")
        return None
    else:
        run(f"mkdir -p {OUTPUT_PATH}/{base_commit_sha}/{head_commit_sha}")
        dest_path = f"{base_commit_sha}/{head_commit_sha}/diff-{suffix}.html"
        whitespace_context = "" if whitespace else "(ignoring whitespace)"
        with tempfile.NamedTemporaryFile() as tmp_file:
            write_html_template(title, f"rev. {head_commit_sha} {whitespace_context}", tmp_file)

            # Generate HTML diff. This uses the diff2html-cli, which defers to `git diff` under the hood.
            # All arguments after the first `--` go to the `git diff` command.
            diff_cmd = f"diff2html -s line -f html -d word -i command --hwt "\
                f"{tmp_file.name} -F {OUTPUT_PATH}/{dest_path} -- "\
                f"-U20 {whitespace_flag} {BASE_BRANCH_NAME} {HEAD_BRANCH_NAME} -- {path_to_diff}"
            eprint(f"Running diff cmd: {diff_cmd}")
            run(diff_cmd)
        return dest_path


def diff_link(diff_text, empty_diff_text, diff_location, alternate_text, alternate_location):
    if diff_location is None:
        return empty_diff_text
    else:
        return f"[{diff_text}]({CDN_URL}/codegen-diff/{diff_location}) ([{alternate_text}]({CDN_URL}/codegen-diff/{alternate_location}))"


def make_diffs(base_commit_sha, head_commit_sha):
    sdk_ws = make_diff("AWS SDK", f"{OUTPUT_PATH}/aws-sdk", base_commit_sha,
                       head_commit_sha, "aws-sdk", whitespace=True)
    sdk_nows = make_diff("AWS SDK", f"{OUTPUT_PATH}/aws-sdk", base_commit_sha, head_commit_sha,
                         "aws-sdk-ignore-whitespace", whitespace=False)
    server_ws = make_diff("Server Test", f"{OUTPUT_PATH}/codegen-server-test", base_commit_sha,
                          head_commit_sha, "server-test", whitespace=True)
    server_nows = make_diff("Server Test", f"{OUTPUT_PATH}/codegen-server-test", base_commit_sha,
                            head_commit_sha, "server-test-ignore-whitespace", whitespace=False)
    server_ws_python = make_diff("Server Test Python", f"{OUTPUT_PATH}/codegen-server-test-python", base_commit_sha,
                                 head_commit_sha, "server-test-python", whitespace=True)
    server_nows_python = make_diff("Server Test Python", f"{OUTPUT_PATH}/codegen-server-test-python", base_commit_sha,
                                   head_commit_sha, "server-test-python-ignore-whitespace", whitespace=False)

    sdk_links = diff_link('AWS SDK', 'No codegen difference in the AWS SDK',
                          sdk_ws, 'ignoring whitespace', sdk_nows)
    server_links = diff_link('Server Test', 'No codegen difference in the Server Test',
                             server_ws, 'ignoring whitespace', server_nows)
    server_links_python = diff_link('Server Test Python', 'No codegen difference in the Server Test Python',
                                    server_ws_python, 'ignoring whitespace', server_nows_python)
    # Save escaped newlines so that the GitHub Action script gets the whole message
    return "A new generated diff is ready to view.\\n"\
        f"- {sdk_links}\\n"\
        f"- {server_links}\\n"\
        f"- {server_links_python}\\n"


def write_to_file(path, text):
    with open(path, "w") as file:
        file.write(text)


# Prints to stderr
def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)


# Runs a shell command
def run(command, shell=False):
    if not shell:
        command = shlex.split(command)
    subprocess.run(command, stdout=sys.stderr, stderr=sys.stderr, shell=shell, check=True)


# Returns the output from a shell command. Bails if the command failed
def get_cmd_output(command):
    result = subprocess.run(shlex.split(command), capture_output=True, check=True)
    return result.stdout.decode("utf-8").strip()


# Runs a shell command and returns its exit status
def get_cmd_status(command):
    return subprocess.run(command, capture_output=True, shell=True).returncode


if __name__ == "__main__":
    main()
