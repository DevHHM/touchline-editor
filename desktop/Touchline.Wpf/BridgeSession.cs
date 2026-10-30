using System.Text.Json;

namespace Touchline.Wpf;

/// <summary>Retries engine startup without replacing a live, possibly writing worker.</summary>
public sealed class BridgeSession
{
 public BridgeClient? Client { get; private set; }

 public async Task<JsonElement> EnsureReadyAsync(Func<string> resolveEngine, Action<string>? diagnostic = null)
 {
  if (Client == null || Client.HasExited)
  {
   if (Client != null) await Client.DisposeAsync();
   Client = null;
   try { Client = new BridgeClient(resolveEngine()); }
   catch(Exception error) { DiagnosticsLog.Current?.Failure("lifecycle","engine_start_failed",error); throw; }
   if (diagnostic != null) Client.Diagnostic += diagnostic;
  }
  // A failed/broken response from a running worker is not permission to kill it.
  // Only status is sent here; mutations are never repeated by recovery.
  return await Client.CallAsync("status");
 }
}
