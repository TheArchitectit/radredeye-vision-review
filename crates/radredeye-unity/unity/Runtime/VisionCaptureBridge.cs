using System.Collections;
using UnityEngine;
using UnityEngine.Networking;

/// <summary>
/// Captures the Unity viewport as PNG and POSTs it to the radredeye daemon bridge.
///
/// Usage:
///   1. Add this MonoBehaviour to a GameObject in your scene.
///   2. Set `bridgeUrl` to your daemon endpoint (default: http://localhost:8765/capture).
///   3. Frames are sent every `captureIntervalSeconds` while the component is enabled.
/// </summary>
public class RadredeyeCaptureBridge : MonoBehaviour
{
    [Tooltip("URL of the radredeye daemon bridge /capture endpoint.")]
    public string bridgeUrl = "http://localhost:8765/capture";

    [Tooltip("Seconds between captures. 0 = every frame (not recommended).")]
    [Min(0.05f)]
    public float captureIntervalSeconds = 1.0f;

    [Tooltip("If true, also save PNGs to Application.persistentDataPath/screenshots/")]
    public bool saveLocally = false;

    private bool _capturing;

    private void OnEnable()
    {
        _capturing = true;
        StartCoroutine(CaptureLoop());
    }

    private void OnDisable()
    {
        _capturing = false;
    }

    private IEnumerator CaptureLoop()
    {
        while (_capturing)
        {
            yield return new WaitForEndOfFrame();

            // Capture the screen as a Texture2D.
            var tex = new Texture2D(Screen.width, Screen.height, TextureFormat.RGBA32, false);
            tex.ReadPixels(new Rect(0, 0, Screen.width, Screen.height), 0, 0);
            tex.Apply();

            byte[] png = tex.EncodeToPNG();
            Destroy(tex);

            if (saveLocally)
            {
                string dir = Application.persistentDataPath + "/screenshots";
                System.IO.Directory.CreateDirectory(dir);
                string path = dir + "/frame_" + System.DateTime.Now.ToString("yyyyMMdd_HHmmss_fff") + ".png";
                System.IO.File.WriteAllBytes(path, png);
            }

            // POST to daemon bridge.
            yield return StartCoroutine(PostPng(png));

            if (captureIntervalSeconds > 0f)
                yield return new WaitForSeconds(captureIntervalSeconds);
        }
    }

    private IEnumerator PostPng(byte[] png)
    {
        using var request = new UnityWebRequest(bridgeUrl, "POST");
        request.uploadHandler = new UploadHandlerRaw(png);
        request.downloadHandler = new DownloadHandlerBuffer();
        request.SetRequestHeader("Content-Type", "image/png");
        request.timeout = 5;

        yield return request.SendWebRequest();

        if (request.result != UnityWebRequest.Result.Success)
        {
            Debug.LogWarning($"[RadredeyeCaptureBridge] POST failed: {request.error}");
        }
    }
}
