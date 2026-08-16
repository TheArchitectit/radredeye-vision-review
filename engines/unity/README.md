# Unity adapter

Planned.  
Likely shape: a C# `MonoBehaviour` that reads a `RenderTexture` every N seconds and forwards the RGBA bytes to the radredeye `CapturePipeline` via a native plugin or gRPC sidecar.
