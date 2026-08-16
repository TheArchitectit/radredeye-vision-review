// VisionCaptureComponent.h
// Unreal Engine component that captures the viewport as PNG and POSTs it to
// the Vision Enabler daemon bridge.
//
// Usage:
//   1. Add UVisionCaptureComponent to any Actor in your level.
//   2. Set BridgeUrl to your daemon endpoint (default: http://localhost:8765/capture).
//   3. Set CaptureIntervalSeconds (default: 1.0).
//   4. Enable the component — captures begin automatically.

#pragma once

#include "CoreMinimal.h"
#include "Components/ActorComponent.h"
#include "Http.h"
#include "VisionCaptureComponent.generated.h"

UCLASS(ClassGroup = (VisionEnabler), meta = (BlueprintSpawnableComponent))
class VISIONENABLERUNREAL_API UVisionCaptureComponent : public UActorComponent
{
    GENERATED_BODY()

public:
    UVisionCaptureComponent();

    /** URL of the Vision Enabler daemon bridge /capture endpoint. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Vision Enabler")
    FString BridgeUrl = TEXT("http://localhost:8765/capture");

    /** Seconds between captures. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Vision Enabler", meta = (ClampMin = "0.05"))
    float CaptureIntervalSeconds = 1.0f;

    /** If true, also save PNGs to the project's Saved/screenshots/ directory. */
    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "Vision Enabler")
    bool bSaveLocally = false;

protected:
    virtual void BeginPlay() override;
    virtual void EndPlay(const EEndPlayReason::Type EndPlayReason) override;
    virtual void TickComponent(float DeltaTime, ELevelTick TickType,
                               FActorComponentTickFunction* ThisTickFunction) override;

private:
    float TimeSinceLastCapture = 0.0f;
    void CaptureFrame();
    void PostPng(const TArray<uint8>& PngData);
};
