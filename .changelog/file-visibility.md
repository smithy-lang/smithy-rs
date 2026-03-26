---
applies_to:
  - client
authors:
  - lnj
references:
breaking: true
new_feature: false
bug_fix: true
---

Now files written by the SDK (like credential caches) are created with file
permissions `0o600` on unix systems. This could break customers who were relying
on the visibility of those files to other users on the system.
