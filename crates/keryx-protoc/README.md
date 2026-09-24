# keryx-protoc

The keryx `protoc`/`buf` plugin: the `protoc-gen-keryx` binary, a bytes-to-bytes `CodeGeneratorRequest` → `CodeGeneratorResponse` frontend over [`keryx-core`](../keryx-core/). `protoc` and `buf` discover a plugin by binary name, so the binary is `protoc-gen-keryx` while the crate keeps keryx's `keryx-*` voice.

See **the keryx Book** ([`docs/book/`](../../docs/book/)) for the translation it performs.
