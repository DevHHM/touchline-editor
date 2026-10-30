using System.Text.Json;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed class LiveConnectionObservation
{
 [JsonPropertyName("state")] public string State { get; set; } = "disabled";
 [JsonPropertyName("monitoring")] public bool Monitoring { get; set; }
 [JsonPropertyName("changed")] public bool Changed { get; set; }
 [JsonPropertyName("can_reconnect")] public bool CanReconnect { get; set; }
 [JsonPropertyName("generation")] public long Generation { get; set; }
 [JsonPropertyName("session_generation")] public long? SessionGeneration { get; set; }
 [JsonPropertyName("data_revision")] public long? DataRevision { get; set; }
 [JsonPropertyName("date_changed")] public bool DateChanged { get; set; }
 [JsonPropertyName("game_date")] public string? GameDate { get; set; }
 [JsonPropertyName("reason")] public string Reason { get; set; } = "";
 [JsonPropertyName("summary")] public JsonElement Summary { get; set; }
}

public readonly record struct LiveReloadDecision(bool Invalidate, bool Reconnect, bool RefreshDate, bool Stop, bool RestoreConnected = false);

/// <summary>UI-only policy. A new session never reuses drafts or preview tokens.</summary>
public sealed class LiveReloadState
{
 public bool Enabled { get; private set; }
 public bool Invalidated { get; private set; }
 public long? Generation { get; private set; }
 public long? SessionGeneration { get; private set; }
 public void Enable() { Enabled = true; Invalidated = false; Generation = SessionGeneration = null; }
 public void Disable() { Enabled = false; Invalidated = false; Generation = SessionGeneration = null; }
 public void InvalidateView() { if(Enabled) Invalidated=true; }
 public LiveReloadDecision Observe(LiveConnectionObservation value)
 {
  if (!Enabled) return default;
  if (!value.Monitoring || value.State is "snapshot" or "disabled") { Disable(); return new(false, false, false, true); }
  // Older engines use generation for edit revisions too. A connected response
  // with Changed=false must not discard the current view after a normal apply.
  bool different = SessionGeneration.HasValue && value.SessionGeneration.HasValue && SessionGeneration.Value != value.SessionGeneration.Value;
  Generation = value.Generation;
  SessionGeneration = value.SessionGeneration;
  bool unavailable = value.State is "loading" or "waiting" or "ready" or "error" or "unsupported_build";
  bool invalidate = !Invalidated && value.State != "reconnected" && (value.Changed || different || unavailable);
  if (invalidate) Invalidated = true;
  bool reconnect = value.State == "ready" && value.CanReconnect;
  // The engine may have reconnected between timer ticks. Restore its current
  // summary rather than waiting for a ready state which will never arrive.
  bool restoreConnected = Invalidated && value.State == "connected";
  if (value.State == "reconnected") Invalidated = false;
  return new(invalidate, reconnect, !Invalidated && value.State == "connected" && value.DateChanged, false, restoreConnected);
 }
}
