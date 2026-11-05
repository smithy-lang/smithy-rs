---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["Regalias3"]
references: ["smithy-rs#2833"]
breaking: false
new_feature: false
bug_fix: true
---
`ByteStream::from_path` and `ByteStream::read_from()` now read files with a 64 KiB buffer by default, up from 4 KiB. On a 64 MiB file this makes reads about 5x faster (174 to 899 MiB/s in the new `bytestream_from_file` benchmark). Each open file stream uses about 60 KiB more memory. To keep the old behavior, call `ByteStream::read_from().buffer_size(4096)`.
