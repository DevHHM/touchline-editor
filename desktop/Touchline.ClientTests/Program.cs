using System.Globalization;
using System.Text;
using System.Text.Json;
using Touchline.Wpf;

if (args.Contains("--bridge"))
{
    Console.InputEncoding = new UTF8Encoding(false);
    Console.OutputEncoding = new UTF8Encoding(false);
    while (Console.ReadLine() is { } line)
    {
        using var input = JsonDocument.Parse(line);
        var root = input.RootElement;
        var id = root.GetProperty("id").GetInt64();
        var command = root.GetProperty("command").GetString();
        if (command == "delay") await Task.Delay(180);
        if (command == "uncertain_write")
        {
            Console.WriteLine("{}");
            await Task.Delay(2700);
            File.WriteAllText(root.GetProperty("args").GetProperty("marker").GetString()!, "completed");
            continue;
        }
        if (command == "malformed") { Console.WriteLine("{}"); continue; }
        if (command == "exit") return 0;
        if (command == "diagnostic") Console.Error.WriteLine("테스트 진단");
        if(command=="connect"&&root.GetProperty("args").TryGetProperty("failForDiagnostics",out _)) {
            Console.WriteLine(JsonSerializer.Serialize(new{id,ok=false,error="FM 접근 실패 0x12345678 token=backend-secret",data=(object?)null}));continue;
        }
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, ok = command != "fail", error = command == "fail" ? "테스트 거절" : null,
            data = new { command, args = root.GetProperty("args").Clone() }
        }));
    }
    return 0;
}

var passed = new List<string>();
void Check(bool condition, string name)
{
    if (!condition) throw new Exception("FAILED: " + name);
    passed.Add(name);
}

if(args.Contains("--sharing-tests")) {
    await DiagnosticSharingTests.RunAsync(Check);
    Console.WriteLine(JsonSerializer.Serialize(new{passed=passed.Count,failed=0,tests=passed,fm_accessed=false}));
    return 0;
}

var reload = new LiveReloadState();
var connectedObservation = new LiveConnectionObservation { State = "connected", Monitoring = true, Generation = 7 };
Check(reload.Observe(connectedObservation) == default && !reload.Enabled, "reload_disabled_never_polls_or_reconnects");
reload.Enable();
Check(reload.Observe(connectedObservation) == default && reload.Generation == 7, "reload_initial_connection_keeps_profile");
Check(reload.Observe(new() { State = "connected", Monitoring = true, Generation = 7, DateChanged = true }).RefreshDate && !reload.Invalidated, "reload_day_advance_keeps_unsaved_draft");
var lostDecision = reload.Observe(new() { State = "loading", Monitoring = true, Generation = 8 });
Check(lostDecision.Invalidate && !lostDecision.Reconnect && reload.Invalidated, "reload_generation_change_invalidates_without_changed_flag");
Check(!reload.Observe(new() { State = "waiting", Monitoring = true, Generation = 8 }).Invalidate, "reload_repeated_wait_does_not_discard_twice");
Check(!reload.Observe(new() { State = "ready", Monitoring = true, Generation = 8, CanReconnect = false }).Reconnect, "reload_requires_backend_stable_ready");
Check(reload.Observe(new() { State = "ready", Monitoring = true, Generation = 8, CanReconnect = true }).Reconnect, "reload_stable_ready_requests_one_reconnect");
reload.Observe(new() { State = "reconnected", Monitoring = true, Generation = 9 });
Check(!reload.Invalidated && reload.Enabled && reload.Generation == 9, "reload_commit_clears_invalidated_state");
Check(reload.Observe(new() { State = "snapshot", Monitoring = false, Generation = 10 }).Stop && !reload.Enabled, "reload_manual_snapshot_stops_live_monitor");
Check(!reload.Observe(new() { State = "ready", Monitoring = true, CanReconnect = true, Generation = 11 }).Reconnect, "reload_snapshot_never_reconnects_itself");
reload.Enable();
Check(reload.Observe(new() { State = "waiting", Monitoring = true, Generation = 12 }).Invalidate && reload.Enabled, "reload_initial_failed_connect_keeps_retry_enabled");
reload.Disable();
Check(reload.Generation == null && !reload.Invalidated && !reload.Enabled, "reload_stop_clears_lifecycle_state");
LiveReloadTests.Run(Check);
await ConnectionRecoveryTests.RunAsync(Check);
PositionSearchTests.Run(Check);
await DiagnosticsTests.RunAsync(Check);
await DiagnosticSharingTests.RunAsync(Check);
GameEditionTests.Run(Check);
DetailPresentationTests.Run(Check);
PackagePathsTests.Run(Check);
DesktopUsabilityTests.Run(Check);
AbilityVisibilityTests.Run(Check);
await ClubGrowthAvailabilityTests.RunAsync(Check);

var detail = JsonSerializer.Deserialize<DetailRecord>("""
{"entity":{"id":10,"kind":"Player","name":"김민수","english_name":"Kim Min-su","ca":153,"pa":199},"fields":[{"key":"passing","label":"패스","section":"기술","value":13.625,"min":1,"max":20,"step":1,"editable":true}],"notes":[],"scores":[],"city_id":516}
""", JsonDefaults.Options)!;
detail.InitializeFields();
var field = detail.Fields[0];
Check(detail.Entity.DisplayName == "김민수" && detail.Entity.AbilityText == "153 / 199", "entity_utf8_and_helpers");
string[] tacticalRoles = ["GK", "SW", "DR", "DC", "DL", "WBR", "WBL", "DM", "MR", "MC", "ML", "AMR", "AMC", "AML", "ST"];
Check(tacticalRoles.Select(PositionOrder.Rank).SequenceEqual(Enumerable.Range(0, 15)), "position_goalkeeper_to_striker_ranks");
Check(PositionOrder.Rank("ST · AMC / DC, DR\tGK") == 0 && PositionOrder.Rank("AML / MC") == 9 && PositionOrder.Rank("dc · dm") == 3, "position_compound_exact_tokens_and_separators");
Check(new string?[] { null, "", " ", "DMC", "DCM", "AMCfoo", "Goalkeeper" }.All(role => PositionOrder.Rank(role) == PositionOrder.UnknownRank), "position_unknown_not_substring_matched");
var tacticalPeople = tacticalRoles.Reverse().Select((role, index) => new EntityRecord { Id = (uint)(100 + index), Kind = "Player", Role = role }).Concat(new[] { new EntityRecord { Id = 2, Kind = "Player", Role = "미확인" }, new EntityRecord { Id = 1, Kind = "Player", Role = "" } }).ToList();
tacticalPeople.Sort((a, b) => PositionOrder.Compare(a, b));
Check(tacticalPeople.Take(15).Select(person => person.Role).SequenceEqual(tacticalRoles) && tacticalPeople.Skip(15).Select(person => person.Id).SequenceEqual(new uint[] { 1, 2 }), "position_ascending_unknown_last_stable_uid");
tacticalPeople.Sort((a, b) => PositionOrder.Compare(a, b, true));
Check(tacticalPeople.Take(15).Select(person => person.Role).SequenceEqual(tacticalRoles.Reverse()) && tacticalPeople.Skip(15).Select(person => person.Id).SequenceEqual(new uint[] { 1, 2 }), "position_descending_unknown_last_stable_uid");
var tiedPositions = new[] { new EntityRecord { Id = 9, Kind = "Player", Role = "ST / DC" }, new EntityRecord { Id = 3, Kind = "Player", Role = "DC" } };
Check(PositionOrder.Compare(tiedPositions[0], tiedPositions[1]) > 0 && PositionOrder.Compare(tiedPositions[0], tiedPositions[1], true) > 0, "position_compound_rank_tie_uid_both_directions");
var staffRoleA = new EntityRecord { Id = 11, Kind = "Staff", Role = "AMC" }; var staffRoleB = new EntityRecord { Id = 12, Kind = "Staff", Role = "GK" };
Check(PositionOrder.Compare(staffRoleA, staffRoleB) < 0 && PositionOrder.Compare(staffRoleA, staffRoleB, true) > 0, "staff_jobs_keep_lexical_role_sort");
var mixedPositions = tacticalPeople.Concat(new[] { staffRoleA, staffRoleB }).ToList();
bool mixedTransitive = new[] { false, true }.All(descending => mixedPositions.All(a => mixedPositions.All(b => mixedPositions.All(c => PositionOrder.Compare(a, b, descending) > 0 || PositionOrder.Compare(b, c, descending) > 0 || PositionOrder.Compare(a, c, descending) <= 0))));
Check(mixedTransitive && PositionOrder.Compare(tacticalPeople[0], staffRoleA) < 0 && PositionOrder.Compare(tacticalPeople[0], staffRoleA, true) < 0, "mixed_position_staff_comparer_transitive");
PositionFilterTests.Run(Check);
NationalityFilterTests.Run(Check);
SearchConditionTests.Run(Check);
LeagueBrowserTests.Run(Check);
SearchColumnTests.Run(Check);
Check(field.EditText == "14" && field.Value == 13.625 && field.Original == 13.625 && !field.IsDirty, "rounded_display_preserves_fractional_baseline");
field.EditText = field.EditText;
Check(!field.IsDirty && field.Value == 13.625, "unchanged_binding_does_not_mutate");
field.ShowPrecision = true;
Check(field.EditText == "13.625" && !field.IsDirty && field.Value == 13.625, "precision_toggle_preserves_original");
field.ShowPrecision = false;
Check(field.EditText == "14" && !field.IsDirty && field.Value == 13.625, "rounded_toggle_preserves_original");
field.EditText = "15";
Check(field.IsDirty && field.IsValid && field.Value == 15 && field.Original == 13.625, "explicit_edit_preserves_original");
field.EditText = "21";
Check(field.IsDirty && !field.IsValid && field.Error.Length > 0, "range_validation");
field.EditText = "NaN";
Check(!field.IsValid, "nonfinite_rejected");
field.Reset();
Check(!field.IsDirty && field.IsValid && field.Value == 13.625, "reset_restores_exact_original");
CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo("fr-FR");
field.EditText = "14.25";
Check(field.IsValid && field.Value == 14.25, "invariant_decimal_input");
field.EditText = "14,25";
Check(!field.IsValid, "locale_decimal_rejected_without_silent_reinterpretation");
field.Reset();
field.Editable = false;
field.EditText = "19";
Check(!field.IsDirty && field.Value == 13.625, "readonly_field_rejects_ui_edits");
field.Editable = true;
field.Adjust(100);
Check(field.Value == 20, "increment_clamped_to_range");

var executable = Environment.ProcessPath!;
await using (var bridge = new BridgeClient(executable))
{
    var result = await bridge.CallAsync("echo", new { name = "레체 • 김민수" });
    Check(result.GetProperty("args").GetProperty("name").GetString() == "레체 • 김민수", "bridge_utf8_roundtrip");
    bool rejected = false;
    try { await bridge.CallAsync("fail"); } catch (BridgeException ex) { rejected = ex.Message == "테스트 거절"; }
    Check(rejected, "backend_error_propagates");
    result = await bridge.CallAsync("echo");
    Check(result.GetProperty("command").GetString() == "echo", "backend_error_does_not_fault_pipe");
    var calls = Enumerable.Range(0, 10).Select(i => bridge.CallAsync("echo", new { index = i })).ToArray();
    var replies = await Task.WhenAll(calls);
    Check(replies.Select((r, i) => r.GetProperty("args").GetProperty("index").GetInt32() == i).All(v => v), "concurrent_call_ids_remain_matched");
    using var cancellation = new CancellationTokenSource();
    var inFlight = bridge.CallAsync("delay", cancellationToken: cancellation.Token);
    await Task.Delay(50);
    cancellation.Cancel();
    result = await inFlight;
    Check(result.GetProperty("command").GetString() == "delay", "inflight_cancel_drains_success_without_killing_worker");
    var first = bridge.CallAsync("delay");
    using var queuedCancellation = new CancellationTokenSource(20);
    bool queuedCancelled = false;
    try { await bridge.CallAsync("echo", cancellationToken: queuedCancellation.Token); }
    catch (OperationCanceledException) { queuedCancelled = true; }
    await first;
    Check(queuedCancelled, "queued_cancel_prevents_request");
    result = await bridge.CallAsync("echo");
    Check(result.GetProperty("command").GetString() == "echo", "cancel_does_not_desynchronize_next_response");
    var diagnostic = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
    bridge.Diagnostic += message => diagnostic.TrySetResult(message);
    await bridge.CallAsync("diagnostic");
    Check(await diagnostic.Task.WaitAsync(TimeSpan.FromSeconds(3)) == "테스트 진단", "stderr_diagnostics_separate_from_protocol");
}

await using (var bridge = new BridgeClient(executable))
{
    bool rejected = false;
    try { await bridge.CallAsync("malformed"); } catch (BridgeException) { rejected = true; }
    Check(rejected, "malformed_response_rejected");
    rejected = false;
    try { await bridge.CallAsync("echo"); } catch (BridgeException) { rejected = true; }
    Check(rejected, "faulted_connection_not_reused");
}

var disposingBridge = new BridgeClient(executable);
var active = disposingBridge.CallAsync("delay");
await Task.Delay(25);
var disposing = disposingBridge.DisposeAsync().AsTask();
var disposingResult = await active;
await disposing;
Check(disposingResult.GetProperty("command").GetString() == "delay", "dispose_drains_active_request_before_closing_stdin");
bool disposedRejected = false;
try { await disposingBridge.CallAsync("echo"); } catch (ObjectDisposedException) { disposedRejected = true; }
Check(disposedRejected, "disposed_client_rejects_new_requests");
var marker = Path.Combine(Path.GetTempPath(), "touchline-client-test-" + Guid.NewGuid().ToString("N"));
var uncertainBridge = new BridgeClient(executable);
try { await uncertainBridge.CallAsync("uncertain_write", new { marker }); } catch (BridgeException) { }
Check(!uncertainBridge.HasExited, "reload_never_restarts_worker_with_uncertain_inflight_write");
await uncertainBridge.DisposeAsync();
await Task.Delay(900);
Check(File.Exists(marker) && File.ReadAllText(marker) == "completed", "unknown_transaction_outcome_never_forcekills_worker");
File.Delete(marker);
await using (var exitedBridge = new BridgeClient(executable))
{
    try { await exitedBridge.CallAsync("exit"); } catch (BridgeException) { }
    for (int retry = 0; retry < 20 && !exitedBridge.HasExited; retry++) await Task.Delay(25);
    Check(exitedBridge.HasExited, "reload_can_identify_actually_exited_worker");
}
var engineOption = Array.IndexOf(args, "--engine");
var snapshotOption = Array.IndexOf(args, "--snapshot");
if (engineOption >= 0 && snapshotOption >= 0)
{
    await using var engine = new BridgeClient(args[engineOption + 1]);
    var summary = await engine.CallAsync("load_snapshot", new { path = Path.GetFullPath(args[snapshotOption + 1]) });
    Check(!summary.GetProperty("live").GetBoolean() && summary.GetProperty("connected").GetBoolean(), "real_engine_snapshot_readonly_connection");
    var search = await engine.CallAsync("search", new { kind = "Staff", query = "Baldini", limit = 10 });
    Check(search.GetProperty("total").GetInt32() > 0, "real_engine_search");
    var payload = await engine.CallAsync("detail", new { kind = "Staff", id = 4007 });
    var actualDetail = payload.Deserialize<DetailRecord>(JsonDefaults.Options)!;
    actualDetail.InitializeFields();
    Check(actualDetail.Entity.Id == 4007 && actualDetail.Fields.Count > 20 && actualDetail.Fields.All(f => !f.IsDirty), "real_engine_detail_dto_preserves_originals");
    Check(actualDetail.Scores.ValueKind == JsonValueKind.Array && actualDetail.Scores.GetArrayLength() >= 20, "real_engine_staff_scores");
    var editable = actualDetail.Fields.First(f => f.Editable && f.Value < f.Max);
    var nextValue = Math.Min(editable.Max, editable.Value + Math.Max(1, editable.Step));
    var preview = await engine.CallAsync("preview", new { kind = "Staff", id = 4007, edits = new[] { new { key = editable.Key, value = nextValue } } });
    Check(!preview.GetProperty("can_apply").GetBoolean() && preview.GetProperty("changes").GetArrayLength() == 1, "real_engine_offline_preview_readonly");
    bool offlineApplyRejected = false;
    try { await engine.CallAsync("apply", new { token = preview.GetProperty("token").GetString() }); }
    catch (BridgeException) { offlineApplyRejected = true; }
    Check(offlineApplyRejected, "real_engine_offline_apply_rejected");
}
Console.WriteLine(JsonSerializer.Serialize(new { passed = passed.Count, failed = 0, tests = passed, fm_accessed = false }));
return 0;
