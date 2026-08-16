fn main() {
    #[cfg(feature = "grpc-sink")]
    {
        tonic_build::compile_protos("../../proto/radredeye.proto").unwrap(); // guardrails-allow PREVENT-013: build script; proto compile failure is fatal anyway
    }
}
