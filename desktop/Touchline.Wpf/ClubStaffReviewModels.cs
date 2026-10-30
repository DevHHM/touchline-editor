using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed class StaffReviewComponent
{
 [JsonPropertyName("label")] public string Label { get; set; } = "";
 [JsonPropertyName("value")] public JsonElement Value { get; set; }
 [JsonPropertyName("weight")] public double? Weight { get; set; }
 [JsonPropertyName("contribution")] public double? Contribution { get; set; }
 public string DisplayValue => Value.ValueKind switch { JsonValueKind.Number => Value.GetDouble().ToString("0.#", CultureInfo.InvariantCulture), JsonValueKind.String => Value.GetString() ?? "—", JsonValueKind.True => "예", JsonValueKind.False => "아니오", _ => "—" };
}
public sealed class StaffReviewCandidate
{
 [JsonPropertyName("person")] public EntityRecord Person { get; set; } = new();
 [JsonPropertyName("score")] public double Score { get; set; }
 [JsonPropertyName("improvement")] public double Improvement { get; set; }
 [JsonPropertyName("reputation")] public double? Reputation { get; set; }
 [JsonPropertyName("club_reputation")] public double? ClubReputation { get; set; }
 [JsonPropertyName("reputation_gap")] public double? ReputationGap { get; set; }
 [JsonPropertyName("availability")] public string Availability { get; set; } = "";
 [JsonPropertyName("availability_label")] public string AvailabilityLabel { get; set; } = "";
 [JsonPropertyName("reasons")] public List<string> Reasons { get; set; } = [];
 [JsonPropertyName("weekly_wage")] public double? WeeklyWage { get; set; }
 [JsonPropertyName("contract_expires")] public double? ContractExpires { get; set; }
 [JsonPropertyName("role_fit")] public string RoleFit { get; set; } = "unknown";
 [JsonPropertyName("components")] public List<StaffReviewComponent> Components { get; set; } = [];
 public string Name => Person.DisplayName;
 public string ScoreText => StaffReviewFormat.Number(Score, "0.#");
 public string ImprovementText => Improvement.ToString("+0.#;-0.#;0", CultureInfo.InvariantCulture);
 public string ReputationText => StaffReviewFormat.Number(Reputation);
}
public sealed class StaffReviewRow
{
 [JsonPropertyName("person")] public EntityRecord Person { get; set; } = new();
 [JsonPropertyName("role")] public string Role { get; set; } = "";
 [JsonPropertyName("group")] public string Group { get; set; } = "";
 [JsonPropertyName("score_key")] public string ScoreKey { get; set; } = "";
 [JsonPropertyName("score_label")] public string ScoreLabel { get; set; } = "";
 [JsonPropertyName("score")] public double? Score { get; set; }
 [JsonPropertyName("max_score")] public double MaxScore { get; set; } = 300;
 [JsonPropertyName("reputation")] public double? Reputation { get; set; }
 [JsonPropertyName("weekly_wage")] public double? WeeklyWage { get; set; }
 [JsonPropertyName("contract_expires")] public double? ContractExpires { get; set; }
 [JsonPropertyName("status")] public string Status { get; set; } = "pending";
 [JsonPropertyName("status_label")] public string StatusLabel { get; set; } = "평가 전";
 [JsonPropertyName("reasons")] public List<string> Reasons { get; set; } = [];
 [JsonPropertyName("components")] public List<StaffReviewComponent> Components { get; set; } = [];
 [JsonPropertyName("alternatives")] public List<StaffReviewCandidate> Alternatives { get; set; } = [];
 public string Name => Person.DisplayName;
 public string CaText => Person.Ca?.ToString(CultureInfo.InvariantCulture) ?? "—";
 public string ScoreText => StaffReviewFormat.Number(Score, "0.#");
 public string ReputationText => StaffReviewFormat.Number(Reputation);
 public string ReasonSummary => string.Join(" · ", Reasons);
}
public sealed class StaffReviewCoverage
{
 [JsonPropertyName("eligible_candidates")] public int EligibleCandidates { get; set; }
 [JsonPropertyName("selected_candidates")] public int SelectedCandidates { get; set; }
 [JsonPropertyName("scanned_candidates")] public int ScannedCandidates { get; set; }
 [JsonPropertyName("failed_candidates")] public int FailedCandidates { get; set; }
 [JsonPropertyName("method")] public string Method { get; set; } = "";
}
public sealed class StaffReviewReport
{
 [JsonPropertyName("club")] public EntityRecord Club { get; set; } = new();
 [JsonPropertyName("club_reputation")] public double? ClubReputation { get; set; }
 [JsonPropertyName("coverage")] public StaffReviewCoverage Coverage { get; set; } = new();
 [JsonPropertyName("rows")] public List<StaffReviewRow> Rows { get; set; } = [];
 [JsonPropertyName("warnings")] public List<string> Warnings { get; set; } = [];
}
public sealed class StaffReviewProgress
{
 [JsonPropertyName("review_id")] public ulong ReviewId { get; set; }
 [JsonPropertyName("scanned")] public int Scanned { get; set; }
 [JsonPropertyName("total")] public int Total { get; set; }
 [JsonPropertyName("failed")] public int Failed { get; set; }
 [JsonPropertyName("complete")] public bool Complete { get; set; }
 [JsonPropertyName("cancelled")] public bool Cancelled { get; set; }
 [JsonPropertyName("phase")] public string Phase { get; set; } = "";
 [JsonPropertyName("report")] public StaffReviewReport? Report { get; set; }
}
internal static class StaffReviewFormat
{
 public static string Number(double? value, string format = "#,0") => value?.ToString(format, CultureInfo.InvariantCulture) ?? "—";
 public static string Wage(double? value) => value.HasValue ? "£" + Number(value) + " /주" : "확인되지 않음";
 public static string Date(double? value)
 {
  if (!value.HasValue) return "확인되지 않음";
  string digits = value.Value.ToString("0", CultureInfo.InvariantCulture);
  return DateTime.TryParseExact(digits, "yyyyMMdd", CultureInfo.InvariantCulture, DateTimeStyles.None, out var date) ? date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) : "확인되지 않음";
 }
}
