# keryx-test-support

Dev-only helpers shared across keryx's test suites: compile a fixture `.proto` to a `FileDescriptorSet` (keeping its custom-option bytes), and hand-build wire payloads and adversarial descriptor sets that `protoc` would never emit. It is a dev-dependency, not published, and not part of keryx's public surface.
