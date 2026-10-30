using System.Text.Json;

namespace Touchline.Wpf;

/// The backend capability, not the displayed edition, authorizes the query.
public sealed record ClubGrowthAvailability(bool CanRequest, string UnavailableMessage)
{
 public static ClubGrowthAvailability Unavailable { get; } = new(false, "현재 연결에서 성장 참고치 지원 여부를 확인하지 못했습니다. 선수 명단과 CA·PA는 그대로 표시합니다.");

 public static ClubGrowthAvailability FromSummary(JsonElement summary)
 {
  static bool True(JsonElement value, string name) => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var field) && field.ValueKind == JsonValueKind.True;
  if (!True(summary, "connected")) return Unavailable;
  if (!True(summary, "live")) return new(false, "스냅샷에서는 성장 참고치를 새로 조회하지 않습니다. 저장된 선수 명단과 CA·PA를 표시합니다.");
  if (summary.TryGetProperty("build_capabilities", out var capabilities) && True(capabilities, "growth_estimates")) return new(true, "");
  string edition = GameEditionPresentation.FromSummary(summary);
  return new(false, edition == "FM27"
   ? "FM27에서는 성장 참고치 조회를 아직 지원하지 않습니다. 선수 명단과 CA·PA는 정상적으로 조회할 수 있습니다."
   : Unavailable.UnavailableMessage);
 }

 public Func<CancellationToken, Task<ClubGrowthResponse>>? GateFetcher(Func<CancellationToken, Task<ClubGrowthResponse>> fetch) => CanRequest ? fetch : null;
}
