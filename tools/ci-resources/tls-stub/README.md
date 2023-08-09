# TLS Stub

This package is used to verify the client's TLS configuration.

It is used in a CI test. See `ci-tls.yml`, "Verify client TLS configuration".

The stub loads a root certificate authority and uses it to connect to a supplied port on localhost.
`trytls` reads the output on the console and uses the exit code of the stub to pass or fail a test case.
