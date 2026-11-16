---
applies_to: ["client"]
authors: ["aajtodd"]
references: ["smithy-rs#4905"]
breaking: false
new_feature: false
bug_fix: true
---
Upgrade `HickoryDnsResolver` to `hickory-resolver` 0.26.3, which resolves the RUSTSEC advisories reported against `hickory-proto` 0.25 and `hickory-resolver` 0.25. The resolver keeps querying IPv4 first with IPv6 as a fallback, keeps its default cache size of 32, and sends EDNS(0) only when `/etc/resolv.conf` enables it. With `hickory-resolver` 0.26, UDP queries are retransmitted while a response is outstanding, the lookup timeout bounds all nameservers queried in one attempt, and on Apple platforms the system configuration is read from the System Configuration framework instead of `/etc/resolv.conf`.
