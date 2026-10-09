---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

The schema server streaming-input path yields after 32 consecutive ready HTTP body
polls, including empty data frames and trailers. This lets other tasks run while
event streams or streaming blobs receive immediately available chunks.

The HTTP server runtime provides body wrappers with either the default budget or
an explicit positive chunk budget. Frames, errors, end-of-stream state, and size
hints are preserved.
