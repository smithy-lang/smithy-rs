Test base archive
=================

The `test_base.git.tar.gz` is an archived test git repository that looks like a 
smithy-rs repo (with only the runtime crates), with some version numbers and 
release tags.

It is a bare git repository (no working tree). To modify it, use the Makefile in
this directory to unpack it with `make unpack`. This will create a test_base directory
in test_data where you can make any changes you need to the repo. Then running
`make pack` will convert that modified repository back into the archive.

When making new test cases that need some extensive setup, it's best to create
a test-case specific branch in the test base.