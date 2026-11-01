using System.Text.Json;

namespace Touchline.Wpf;

/// <summary>Retries engine startup without replacing a live, possibly writing worker.</summary>
public sealed class BridgeSession
{
 private readonly SemaphoreSlim _readyLock = new(1, 1);
 public BridgeClient? Client { get; private set; }

 public async Task<JsonElement> EnsureReadyAsync(Func<string> resolveEngine, Action<string>? diagnostic = null)
 {
  await _readyLock.WaitAsync().ConfigureAwait(false);
  try
  {
   if (Client == null || Client.HasExited)
   {
    // Draining an exited worker's diagnostics can yield. Keep replacement and
    // its status handshake under one lock so another recovery caller cannot
    // start a second worker or lose ownership of the first replacement.
    if (Client != null) await Client.DisposeAsync().ConfigureAwait(false);
    Client = null;
    try { Client = new BridgeClient(resolveEngine()); }
    catch(Exception error) { DiagnosticsLog.Current?.Failure("lifecycle","engine_start_failed",error); throw; }
    if (diagnostic != null) Client.Diagnostic += diagnostic;
   }
   // A failed/broken response from a running worker is not permission to kill it.
   // Only status is sent here; mutations are never repeated by recovery.
   return await Client.CallAsync("status").ConfigureAwait(false);
  }
  finally { _readyLock.Release(); }
 }
}
