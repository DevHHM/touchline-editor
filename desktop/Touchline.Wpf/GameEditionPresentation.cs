using System.Text.Json;

namespace Touchline.Wpf;

/// Presentation only. The backend fingerprint gate decides which operations
/// are supported; an edition label never grants read or write capability.
public static class GameEditionPresentation
{
 public static string FromProfile(string? profile)
 {
  var token = profile?.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).FirstOrDefault();
  return token is "FM26" or "FM27" ? token : "FM";
 }

 public static string FromSummary(JsonElement summary)
 {
  if (summary.ValueKind != JsonValueKind.Object || !summary.TryGetProperty("connected", out var connected) || connected.ValueKind != JsonValueKind.True)
   return "FM";
  return summary.TryGetProperty("profile", out var profile) && profile.ValueKind == JsonValueKind.String
   ? FromProfile(profile.GetString()) : "FM";
 }
}
