using System.Text.Json;
using Touchline.Wpf;

internal static class ClubGrowthAvailabilityTests
{
 internal static async Task RunAsync(Action<bool,string> check)
 {
  static ClubGrowthAvailability Parse(string json) { using var doc=JsonDocument.Parse(json); return ClubGrowthAvailability.FromSummary(doc.RootElement); }
  var fm27=Parse("""{"connected":true,"live":true,"profile":"FM27 27.0.0","build_capabilities":{"growth_estimates":false}}""");
  int requests=0;
  Func<CancellationToken,Task<ClubGrowthResponse>> fetch=token=>{token.ThrowIfCancellationRequested();requests++;return Task.FromResult(new ClubGrowthResponse {ClubId=722});};
  var blocked=fm27.GateFetcher(fetch);
  if(blocked!=null)await blocked(CancellationToken.None);
  check(!fm27.CanRequest && requests==0 && fm27.UnavailableMessage.Contains("FM27") && fm27.UnavailableMessage.Contains("아직 지원하지"),"fm27_unsupported_growth_never_dispatches_and_explains_support_limit");
  var fm26=Parse("""{"connected":true,"live":true,"profile":"FM26 26.3.2","build_capabilities":{"growth_estimates":true}}""");
  var allowed=fm26.GateFetcher(fetch);
  var response=await allowed!(CancellationToken.None);
  check(fm26.CanRequest && requests==1 && response.ClubId==722,"verified_live_growth_dispatches_exactly_once_and_preserves_response");
  string[] denied=["null","{}","{\"connected\":true,\"live\":true,\"profile\":\"FM26\"}","{\"connected\":true,\"live\":true,\"build_capabilities\":{\"growth_estimates\":\"true\"}}","{\"connected\":true,\"live\":true,\"build_capabilities\":null}","{\"connected\":false,\"live\":true,\"build_capabilities\":{\"growth_estimates\":true}}"]; 
  check(denied.All(json=>!Parse(json).CanRequest && Parse(json).GateFetcher(fetch)==null),"missing_malformed_or_disconnected_growth_capability_fails_closed");
  var snapshot=Parse("""{"connected":true,"live":false,"profile":"FM26 26.3.2","build_capabilities":{"growth_estimates":true}}""");
  check(!snapshot.CanRequest && snapshot.GateFetcher(fetch)==null && snapshot.UnavailableMessage.Contains("스냅샷"),"snapshot_never_dispatches_live_growth_even_with_saved_capability");
  var future=Parse("""{"connected":true,"live":true,"profile":"FM27 later-verified","build_capabilities":{"growth_estimates":true}}""");
  check(future.CanRequest,"growth_support_uses_backend_capability_not_hardcoded_edition");
  check(!fm27.CanRequest && !Parse("{}").CanRequest && fm26.CanRequest,"new_summary_replaces_growth_support_without_stale_session_inheritance");
 }
}
