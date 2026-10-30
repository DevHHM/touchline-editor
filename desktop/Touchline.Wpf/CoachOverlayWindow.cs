using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

/// <summary>
/// Pinned staff overlay. Polls read-only semantic APIs; native mutations target only
/// this window. No game input, focus changes, memory writes, or auto-profile claims.
/// </summary>
public sealed class CoachOverlayWindow : Window
{
    private readonly BridgeClient _bridge;
    private readonly EntityRecord _staff;
    private readonly uint _pid;
    private readonly DateTime _processStarted;
    private readonly DispatcherTimer _monitor;
    private readonly CancellationTokenSource _closed = new();
    private readonly StackPanel _content = new();
    private JsonElement _detail;
    private DateTime _fresh = DateTime.MinValue;
    private DateTime _lastRequest = DateTime.MinValue;
    private bool _reading;
    private bool _stopped;
    private nint _handle;
    private string _scoreGroup = "훈련";
    private string _anchor = "오른쪽 위";
    private int _successfulReads;
    public event Action<string>? StatusChanged;

    private CoachOverlayWindow(BridgeClient bridge, EntityRecord staff, uint pid, DateTime processStarted)
    {
        _bridge = bridge; _staff = staff; _pid = pid; _processStarted = processStarted;
        Title = $"Touchline 고정 코치 · {staff.Id}";
        Width = 440; Height = 564; WindowStyle = WindowStyle.None; ResizeMode = ResizeMode.NoResize;
        AllowsTransparency = true; Background = Brushes.Transparent; ShowInTaskbar = false;
        ShowActivated = false; Focusable = false; IsHitTestVisible = false; Topmost = true;
        Foreground = Ink; FontSize = 12; Opacity = .96;
        FontFamily = Application.Current?.TryFindResource("AppFont") as FontFamily ?? new FontFamily("Segoe UI");
        Content = new Border
        {
            CornerRadius = new CornerRadius(12), Padding = new Thickness(17), BorderThickness = new Thickness(1),
            BorderBrush = new SolidColorBrush(Color.FromRgb(52, 77, 83)), Background = new SolidColorBrush(Color.FromRgb(16, 25, 31)), Child = _content
        };
        SourceInitialized += (_, _) =>
        {
            _handle = new WindowInteropHelper(this).Handle;
            var style = GetWindowLongPtrW(_handle, -20).ToInt64();
            SetWindowLongPtrW(_handle, -20, (nint)(style | 0x08000000 | 0x00000020 | 0x00000080));
            HwndSource.FromHwnd(_handle)?.AddHook((nint hwnd, int msg, nint wparam, nint lparam, ref bool handled) =>
            {
                if (msg == 0x21) { handled = true; return (nint)3; } // MA_NOACTIVATE
                if (msg == 0x84) { handled = true; return (nint)(-1); } // HTTRANSPARENT
                return 0;
            });
        };
        _monitor = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(200) };
        _monitor.Tick += Monitor;
        Closed += (_, _) => { _stopped = true; _closed.Cancel(); _monitor.Stop(); };
    }

    public static async Task<CoachOverlayWindow> StartAsync(BridgeClient bridge, EntityRecord staff, CancellationToken cancellationToken = default)
    {
        if (staff.Kind != "Staff") throw new InvalidOperationException("코치 상세 화면에서 스태프를 선택하세요.");
        var status = await bridge.CallAsync("status", null, cancellationToken);
        if (!GetBool(status, "live") || !status.TryGetProperty("pid", out var p) || !p.TryGetUInt32(out var pid) || pid == 0)
            throw new InvalidOperationException("FM에 실시간으로 연결한 뒤 오버레이를 시작하세요.");
        using var process = Process.GetProcessById(checked((int)pid));
        if (process.HasExited) throw new InvalidOperationException("FM 프로세스가 종료되었습니다.");
        var overlay = new CoachOverlayWindow(bridge, staff, pid, process.StartTime.ToUniversalTime());
        try
        {
            // Create a hidden native handle; apply no-activation flags before first show.
            new WindowInteropHelper(overlay).EnsureHandle();
            await overlay.RefreshAsync();
            if (overlay._fresh == DateTime.MinValue) throw new InvalidOperationException("코치의 실시간 정보를 읽지 못했습니다.");
            overlay._monitor.Start(); overlay.Monitor(null, EventArgs.Empty);
            return overlay;
        }
        catch { overlay.Close(); throw; }
    }

    public void SetAnchor(string anchor) { _anchor = anchor; Monitor(null, EventArgs.Empty); }
    public void SetScoreGroup(string group) { _scoreGroup = group; Render(); }

    /// <summary>Optional live QA; samples natural foreground only, never activates or sends FM input.</summary>
    public static async Task<object> VerifyLiveAsync(BridgeClient bridge, EntityRecord staff, CancellationToken cancellationToken = default)
    {
        var foregroundBefore = GetForegroundWindow();
        var overlay = await StartAsync(bridge, staff, cancellationToken);
        var samples = new List<object>();
        int foregroundSamples = 0, visibleSamples = 0, unexpectedVisible = 0, focused = 0;
        try
        {
            for (int i = 0; i < 40; i++)
            {
                await Task.Delay(200, cancellationToken);
                var target = ForegroundTarget(overlay._pid, out var rect, out var scale);
                var visible = IsWindowVisible(overlay._handle);
                if (target) foregroundSamples++;
                if (visible) visibleSamples++;
                if (!target && visible) unexpectedVisible++;
                if (GetForegroundWindow() == overlay._handle) focused++;
                samples.Add(new { fm_foreground = target, visible, fresh_age_ms = (DateTime.UtcNow - overlay._fresh).TotalMilliseconds, dpi_scale = scale });
            }
            var style = GetWindowLongPtrW(overlay._handle, -20).ToInt64();
            return new
            {
                pid = overlay._pid, pinned_staff = staff.Id, duration_seconds = 8,
                fresh_reads = overlay._successfulReads, fm_foreground_samples = foregroundSamples,
                visible_samples = visibleSamples, unexpected_visible_samples = unexpectedVisible, overlay_focused_samples = focused,
                no_activate = (style & 0x08000000) != 0, click_through = (style & 0x20) != 0, tool_window = (style & 0x80) != 0,
                foreground_preserved = GetForegroundWindow() == foregroundBefore,
                foreground_show_verified = foregroundSamples > 0 && visibleSamples > 0,
                game_input = false, game_modified = false, samples
            };
        }
        finally { overlay.Close(); }
    }

    /// <summary>Offline verification: creates only our own hidden HWND and rasterizes supplied data.</summary>
    public static object VerifyNativeShell(BridgeClient bridge, EntityRecord staff, JsonElement detail, string previewPath)
    {
        var foregroundBefore = GetForegroundWindow();
        var overlay = new CoachOverlayWindow(bridge, staff, 0, DateTime.MinValue);
        try
        {
            overlay._detail = detail.Clone(); overlay._fresh = DateTime.UtcNow; overlay.Render();
            new WindowInteropHelper(overlay).EnsureHandle();
            var exStyle = GetWindowLongPtrW(overlay._handle, -20).ToInt64();
            var visual = (FrameworkElement)overlay.Content;
            visual.Measure(new Size(440, 564)); visual.Arrange(new Rect(0, 0, 440, 564)); visual.UpdateLayout();
            var image = new RenderTargetBitmap(440, 564, 96, 96, PixelFormats.Pbgra32); image.Render(visual);
            var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(image));
            using (var stream = System.IO.File.Create(previewPath)) encoder.Save(stream);
            var negative = Placement(new NativeRect { Left = -2560, Top = 0, Right = 0, Bottom = 1440 }, 1, "오른쪽 위");
            var scaled = Placement(new NativeRect { Left = 0, Top = 0, Right = 3840, Bottom = 2160 }, 1.5, "왼쪽 아래");
            return new
            {
                no_activate = (exStyle & 0x08000000) != 0,
                click_through = (exStyle & 0x20) != 0,
                tool_window = (exStyle & 0x80) != 0,
                hidden = !IsWindowVisible(overlay._handle),
                foreground_preserved = GetForegroundWindow() == foregroundBefore,
                rejects_zero_pid = !ForegroundTarget(0, out _, out _),
                negative_coordinates = negative == (-456, 56, 440, 564),
                mixed_dpi_geometry = scaled == (24, 1290, 660, 846),
                attributes = Fields(detail, "fields").Count(x => Str(x, "key").StartsWith("staff_", StringComparison.Ordinal)),
                training_scores = Fields(detail, "scores").Count(x => TrainingKeys.Contains(Str(x, "key"))),
                game_accessed = false, game_input = false, preview = previewPath
            };
        }
        finally { overlay.Close(); }
    }

    private void Monitor(object? sender, EventArgs args)
    {
        if (_stopped) return;
        var now = DateTime.UtcNow;
        if (!_reading && now - _lastRequest >= TimeSpan.FromSeconds(2)) _ = RefreshAsync();
        if (now - _fresh > TimeSpan.FromSeconds(5) || !ForegroundTarget(_pid, out var rect, out var scale))
        {
            if (IsVisible) Hide();
            return;
        }
        var physical = Placement(rect, scale, _anchor);
        if (!IsVisible) Show(); // ShowActivated=false plus WS_EX_NOACTIVATE.
        SetWindowPos(_handle, (nint)(-1), physical.X, physical.Y, physical.Width, physical.Height, 0x0010 | 0x0040);
    }

    private async Task RefreshAsync()
    {
        if (_reading || _stopped) return;
        _reading = true; _lastRequest = DateTime.UtcNow;
        try
        {
            using var process = Process.GetProcessById(checked((int)_pid));
            if (process.HasExited || process.StartTime.ToUniversalTime() != _processStarted) throw new InvalidOperationException("FM 프로세스가 변경되거나 종료되었습니다.");
            var status = await _bridge.CallAsync("status", null, _closed.Token);
            if (!GetBool(status, "live") || !status.TryGetProperty("pid", out var p) || !p.TryGetUInt32(out var pid) || pid != _pid)
                throw new InvalidOperationException("FM 연결이 바뀌었습니다. 코치를 다시 선택하세요.");
            var detail = await _bridge.CallAsync("overlay_detail", new { id = _staff.Id }, _closed.Token);
            if (_stopped) return;
            if (!GetBool(detail, "live") || !detail.TryGetProperty("entity", out var entity) || Str(entity, "kind") != "Staff" || !entity.TryGetProperty("id", out var id) || !id.TryGetUInt32(out var uid) || uid != _staff.Id)
                throw new InvalidOperationException("코치 식별 정보가 일치하지 않습니다.");
            _detail = detail.Clone(); _fresh = DateTime.UtcNow; _successfulReads++; Render();
            StatusChanged?.Invoke($"{_staff.DisplayName} 고정 · 최신 정보 {_fresh.ToLocalTime():HH:mm:ss} · FM이 전경일 때 표시");
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            _fresh = DateTime.MinValue; if (IsVisible) Hide();
            _monitor.Stop(); _stopped = true; StatusChanged?.Invoke($"오버레이 중지: {ex.Message}");
        }
        finally { _reading = false; }
    }

    private void Render()
    {
        if (_detail.ValueKind != JsonValueKind.Object) return;
        _content.Children.Clear();
        _content.Children.Add(new TextBlock { Text = _staff.DisplayName, FontSize = 19, FontWeight = FontWeights.Bold, Foreground = Ink, TextTrimming = TextTrimming.CharacterEllipsis });
        _content.Children.Add(new TextBlock { Text = $"선택한 코치 고정 · {_staff.Club}", FontSize = 11, Foreground = Muted, Margin = new Thickness(0, 4, 0, 13) });
        _content.Children.Add(Heading("코치 능력치"));
        var attributes = Fields(_detail, "fields").Where(x => Str(x, "key").StartsWith("staff_", StringComparison.Ordinal)).ToList();
        _content.Children.Add(ValuesGrid(attributes, 12));
        _content.Children.Add(Heading($"{_scoreGroup} 점수 · 사용자 계산식"));
        var scores = Fields(_detail, "scores").ToList();
        var keys = _scoreGroup == "육성 · 관리" ? ManagementKeys : _scoreGroup == "영입 · 지원" ? SupportKeys : TrainingKeys;
        _content.Children.Add(ValuesGrid(scores.Where(x => keys.Contains(Str(x, "key"))).ToList(), 5));
        _content.Children.Add(new TextBlock { Text = $"읽기 전용 · {_fresh.ToLocalTime():HH:mm:ss} 갱신 · 프로필 자동 감지 아님", FontSize = 10.5, Foreground = Muted, Margin = new Thickness(0, 10, 0, 0) });
    }
    private static TextBlock Heading(string value) => new() { Text = value, Foreground = Accent, FontSize = 11, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 4, 0, 7) };
    private static Grid ValuesGrid(List<JsonElement> fields, int rows)
    {
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition()); grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(16) }); grid.ColumnDefinitions.Add(new ColumnDefinition());
        for (int i = 0; i < rows; i++) grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(23) });
        for (int i = 0; i < Math.Min(fields.Count, rows * 2); i++)
        {
            var field = fields[i]; var row = new DockPanel();
            var value = field.TryGetProperty("value", out var v) && v.TryGetDouble(out var number) ? number : 0;
            var isScore = Str(field, "key").StartsWith("score_", StringComparison.Ordinal);
            var numberText = new TextBlock { Text = value.ToString(isScore ? "0.#" : "0", System.Globalization.CultureInfo.InvariantCulture), ToolTip = value.ToString("G17", System.Globalization.CultureInfo.InvariantCulture), FontSize = 12, FontWeight = FontWeights.Bold, MinWidth = 28, TextAlignment = TextAlignment.Right, Foreground = isScore || value >= 16 ? Accent : value >= 11 ? Gold : Ink };
            DockPanel.SetDock(numberText, Dock.Right); row.Children.Add(numberText);
            row.Children.Add(new TextBlock { Text = Str(field, "label"), Foreground = Ink, FontSize = 11.5, TextTrimming = TextTrimming.CharacterEllipsis });
            Grid.SetRow(row, i % rows); Grid.SetColumn(row, i / rows * 2); grid.Children.Add(row);
        }
        return grid;
    }
    private static readonly HashSet<string> TrainingKeys = ["score_technical_attack", "score_technical_defense", "score_technical_mental", "score_tactical_attack", "score_tactical_defense", "score_tactical_mental", "score_fitness", "score_gk_shot", "score_gk_distribution", "score_set_pieces"];
    private static readonly HashSet<string> ManagementKeys = ["score_assistant", "score_general", "score_reserve_manager", "score_youth_manager", "score_youth_assistant", "score_youth_coach", "score_youth_goalkeeper", "score_head_youth"];
    private static readonly HashSet<string> SupportKeys = ["score_dof", "score_td", "score_loan_manager", "score_scout", "score_head_analyst", "score_performance_analyst", "score_recruitment_analyst", "score_physio", "score_scientist"];
    private static readonly Brush Ink = new SolidColorBrush(Color.FromRgb(231, 238, 244));
    private static readonly Brush Muted = new SolidColorBrush(Color.FromRgb(148, 169, 183));
    private static readonly Brush Accent = new SolidColorBrush(Color.FromRgb(90, 216, 172));
    private static readonly Brush Gold = new SolidColorBrush(Color.FromRgb(233, 194, 111));
    private static string Str(JsonElement e, string key) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(key, out var value) ? value.ToString() : "";
    private static bool GetBool(JsonElement e, string key) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.True;
    private static IEnumerable<JsonElement> Fields(JsonElement e, string key) => e.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.Array ? value.EnumerateArray() : Enumerable.Empty<JsonElement>();

    internal static bool ForegroundTarget(uint expectedPid, out NativeRect rect, out double scale)
    {
        rect = default; scale = 1;
        var foreground = GetForegroundWindow(); if (foreground == 0) return false;
        GetWindowThreadProcessId(foreground, out var pid); if (pid != expectedPid || expectedPid == 0) return false;
        var root = GetAncestor(foreground, 3); var hwnd = root == 0 ? foreground : root;
        GetWindowThreadProcessId(hwnd, out pid);
        if (pid != expectedPid || !IsWindowVisible(hwnd) || IsIconic(hwnd) || !GetWindowRect(hwnd, out rect) || rect.Right - rect.Left < 600 || rect.Bottom - rect.Top < 420) return false;
        scale = Math.Clamp(GetDpiForWindow(hwnd) / 96d, 1, 4); return true;
    }
    internal static (int X, int Y, int Width, int Height) Placement(NativeRect rect, double scale, string anchor)
    {
        var margin = (int)Math.Round(16 * scale);
        var width = Math.Max(1, Math.Min((int)Math.Round(440 * scale), rect.Right - rect.Left - 2 * margin));
        var height = Math.Max(1, Math.Min((int)Math.Round(564 * scale), rect.Bottom - rect.Top - 2 * margin));
        var left = anchor.StartsWith("왼쪽", StringComparison.Ordinal);
        var bottom = anchor.EndsWith("아래", StringComparison.Ordinal);
        var x = left ? rect.Left + margin : rect.Right - margin - width;
        var y = bottom ? rect.Bottom - margin - height : rect.Top + margin + (int)Math.Round(40 * scale);
        return (Math.Clamp(x, rect.Left + margin, rect.Right - margin - width), Math.Clamp(y, rect.Top + margin, rect.Bottom - margin - height), width, height);
    }
    [StructLayout(LayoutKind.Sequential)] internal struct NativeRect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")] private static extern nint GetForegroundWindow();
    [DllImport("user32.dll")] private static extern nint GetAncestor(nint window, uint flags);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(nint window, out uint processId);
    [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool GetWindowRect(nint window, out NativeRect rect);
    [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool IsWindowVisible(nint window);
    [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool IsIconic(nint window);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(nint window);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] private static extern nint GetWindowLongPtrW(nint window, int index);
    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")] private static extern nint SetWindowLongPtrW(nint window, int index, nint value);
    [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool SetWindowPos(nint window, nint insertAfter, int x, int y, int cx, int cy, uint flags);
}
