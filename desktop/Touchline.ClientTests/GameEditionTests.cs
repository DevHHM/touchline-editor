using System.Text.Json;
using Touchline.Wpf;

internal static class GameEditionTests
{
 public static void Run(Action<bool, string> check)
 {
  static string Summary(string json) { using var doc=JsonDocument.Parse(json);return GameEditionPresentation.FromSummary(doc.RootElement); }
  check(Summary("""{"connected":true,"live":true,"profile":"FM27 27.0.0 / fingerprint"}""")=="FM27", "edition_fm27_live_badge");
  check(Summary("""{"connected":true,"live":false,"profile":"FM26 26.3.2 / fingerprint"}""")=="FM26", "edition_fm26_snapshot_badge");
  check(Summary("""{"connected":false,"profile":"FM26 26.3.2 / supported default"}""")=="FM", "edition_offline_does_not_claim_default_connection");
  check(Summary("""{"connected":true,"profile":null}""")=="FM" && Summary("null")=="FM", "edition_legacy_or_missing_summary_is_neutral");
  check(new string?[]{null,"","FM270 test","other FM27","FM26unsupported","unknown"}.All(p=>GameEditionPresentation.FromProfile(p)=="FM"), "edition_requires_exact_profile_token");
  check(GameEditionPresentation.FromProfile(" FM27\t27.0.0 / fingerprint")=="FM27", "edition_whitespace_separator_supported");
 }
}
