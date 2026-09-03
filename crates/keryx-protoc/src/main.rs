//! The keryx protoc/buf plugin — the `protoc-gen-keryx` binary (the crate is `keryx-protoc`, in
//! keryx's own voice; the binary keeps the name protoc/buf require). A skeleton for now; the
//! bytes-to-bytes `CodeGeneratorRequest`/`Response` front end over keryx-core lands with the
//! plugin increment.
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    // Until the plugin increment, refuse honestly rather than exit 0 with an empty response —
    // which protoc reads as a successful, empty CodeGeneratorResponse (a gate that lies).
    eprintln!("keryx: the protoc plugin is not implemented yet");
    std::process::ExitCode::FAILURE
}
