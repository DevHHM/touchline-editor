using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Media;

namespace Touchline.Wpf;

/// <summary>Native scenario authoring and the guarded Rust transaction journal.</summary>
public sealed class ToolsWindow : Window
{
    private readonly BridgeClient _bridge;
    private readonly string _mode;
    private readonly uint? _clubId;
    private bool _live;
    private readonly CancellationTokenSource _closing = new();
    private readonly CancellationTokenSource _connectionLost = new();
    private bool _connectionInvalidated;
    private readonly StackPanel _actions = new() { Orientation = Orientation.Horizontal };
    private readonly TextBlock _status = new() { TextWrapping = TextWrapping.Wrap };
    private readonly Grid _body = new();
    private readonly Dictionary<string, TextBox> _inputs = new();
    private readonly ComboBox _sponsors = new() { MinHeight = 58, MaxDropDownHeight = 280, DisplayMemberPath = nameof(SponsorOption.Label) };
    private readonly DataGrid _changes = Table();
    private readonly TextBox _narrative = new() { IsReadOnly = true, TextWrapping = TextWrapping.Wrap, AcceptsReturn = true, BorderThickness = new Thickness(0), Padding = new Thickness(14), VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
    private readonly TextBlock _contextLabel = new() { FontSize = 14, TextWrapping = TextWrapping.Wrap };
    private readonly TextBlock _warnings = new() { TextWrapping = TextWrapping.Wrap, Foreground = Brushes.Goldenrod };
    private readonly List<ReceiptRow> _receipts = new();
    private DataGrid? _history;
    private Button? _previewButton;
    private Button? _saveButton;
    private Button? _applyButton;
    private Button? _undoButton;
    private Button? _overlayStartButton;
    private Button? _overlayStopButton;
    private CoachOverlayWindow? _overlay;
    private EntityRecord? _overlayStaff;
    private string? _token;
    private bool _canApply;
    private bool _busy;
    public bool IsBusy => _busy;
    private bool _loading;
    private JsonElement? _lastPreview;
    public Func<Task<bool>>? BeforeUndo { get; set; }
    public Func<Task>? AfterUndo { get; set; }

    public void InvalidateConnection()
    {
        _connectionInvalidated = true; _live = false; _connectionLost.Cancel();
        _token = null; _canApply = false; _lastPreview = null; _receipts.Clear();
        if (_history != null) _history.ItemsSource = null;
        _changes.ItemsSource = null; _narrative.Clear(); _contextLabel.Text = "연결 해제";
        _overlay?.Close(); _overlay = null; _overlayStaff = null;
        _body.IsEnabled = false; _status.Text = "연결이 해제되었습니다. 다시 연결한 뒤 이 창을 새로 여세요."; RefreshButtons();
    }
    private async Task<JsonElement> CallAsync(string command, object? args = null, CancellationToken cancellationToken = default)
    {
        if (_connectionInvalidated) throw new OperationCanceledException("연결이 해제되었습니다.");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _connectionLost.Token);
        var result = await _bridge.CallAsync(command, args, linked.Token);
        if (_connectionInvalidated) throw new OperationCanceledException("연결이 해제되었습니다.");
        return result;
    }

    public ToolsWindow(BridgeClient bridge, string mode, uint? clubId = null, EntityRecord? staff = null, bool live = false)
    {
        _bridge = bridge;
        _mode = mode;
        _clubId = clubId;
        _live = live;
        Title = mode == "events" ? "이벤트 스튜디오 · Touchline" : mode == "history" ? "변경 기록 · Touchline" : "코치 오버레이 · Touchline";
        Width = 1110; Height = 790; MinWidth = 930; MinHeight = 650;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        FontFamily = Application.Current?.TryFindResource("AppFont") as FontFamily ?? new FontFamily("Segoe UI");
        Background = Brush("BaseBrush", "#10151E"); Foreground = Brush("InkBrush", "#EDF1F7");
        var root = new Grid { Margin = new Thickness(24) };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition());
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var top = new DockPanel { Margin = new Thickness(0, 0, 0, 20) };
        var heading = new StackPanel();
        heading.Children.Add(Text(mode == "events" ? "이벤트 스튜디오" : mode == "history" ? "변경 기록" : "코치 오버레이", 25, true));
        heading.Children.Add(Text(mode == "events" ? "후원 · 구단 지원금 · 지역 개발" : mode == "history" ? "적용 전후의 값과 되돌리기" : "FM 위에서 확인하는 코치 능력치", 13));
        DockPanel.SetDock(_actions, Dock.Right); top.Children.Add(_actions); top.Children.Add(heading);
        root.Children.Add(top); Grid.SetRow(_body, 1); root.Children.Add(_body);
        _status.Margin = new Thickness(0, 14, 0, 0); _status.Foreground = Brush("MutedBrush", "#AAB6C9");
        Grid.SetRow(_status, 2); root.Children.Add(_status); Content = root;
        if (mode == "events") BuildEvents();
        else if (mode == "history") BuildHistory();
        else BuildOverlay(staff);
        Loaded += async (_, _) => await RunAsync(mode == "events" ? LoadEventContextAsync : mode == "history" ? LoadHistoryAsync : () => Task.CompletedTask);
        AbilityVisibility.Current.Changed += AbilityVisibilityChanged;
        Closed += (_, _) => { AbilityVisibility.Current.Changed -= AbilityVisibilityChanged; _closing.Cancel(); _overlay?.Close(); };
    }

    private static Brush Brush(string key, string fallback) => Application.Current?.TryFindResource(key) as Brush ?? new SolidColorBrush((Color)ColorConverter.ConvertFromString(fallback));
    private static TextBlock Text(string text, double size = 13, bool bold = false) => new()
    {
        Text = text, FontSize = size, FontWeight = bold ? FontWeights.Bold : FontWeights.Medium,
        TextWrapping = TextWrapping.Wrap, Foreground = Brush(bold ? "InkBrush" : "MutedBrush", bold ? "#EDF1F7" : "#AAB6C9"), Margin = new Thickness(0, 0, 0, 6)
    };
    private static Border Card(UIElement content, Thickness? margin = null) => new()
    {
        Child = content, Padding = new Thickness(18), Background = Brush("PanelBrush", "#18202C"),
        BorderBrush = Brush("BorderBrush", "#2A3547"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(10), Margin = margin ?? new Thickness(0)
    };
    private static DataGrid Table() => new()
    {
        AutoGenerateColumns = false, IsReadOnly = true, CanUserAddRows = false, CanUserDeleteRows = false,
        HeadersVisibility = DataGridHeadersVisibility.Column, RowHeight = 39, SelectionMode = DataGridSelectionMode.Single,
        GridLinesVisibility = DataGridGridLinesVisibility.None, BorderThickness = new Thickness(0), EnableRowVirtualization = true
    };
    private static void Column(DataGrid grid, string header, string path, double width = 1)
    {
        var textStyle = new Style(typeof(TextBlock), Application.Current?.TryFindResource(typeof(TextBlock)) as Style);
        textStyle.Setters.Add(new Setter(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis));
        textStyle.Setters.Add(new Setter(ToolTipService.ToolTipProperty, new Binding(path)));
        grid.Columns.Add(new DataGridTextColumn { Header = header, Binding = new Binding(path), ElementStyle = textStyle, Width = new DataGridLength(width, DataGridLengthUnitType.Star) });
    }
    private Button Action(string label, Func<Task> callback)
    {
        var button = new Button { Content = label, Margin = new Thickness(8, 2, 0, 2), Padding = new Thickness(14, 9, 14, 9), MinHeight = 38 };
        button.Click += async (_, _) => await RunAsync(callback); _actions.Children.Add(button); return button;
    }

    private void BuildEvents()
    {
        _previewButton = Action("변경 미리보기", PreviewEventAsync);
        _saveButton = Action("시나리오 저장", SaveEventAsync);
        _applyButton = Action("FM에 적용", ApplyEventAsync);
        _body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(405) });
        _body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(16) });
        _body.ColumnDefinitions.Add(new ColumnDefinition());
        var editor = new StackPanel();
        editor.Children.Add(_contextLabel);
        editor.Children.Add(Text("모든 금액은 영국 파운드(GBP)입니다. 후원금과 즉시 지급금은 별개입니다.", 12));
        Section(editor, "시나리오");
        Input(editor, "title", "제목", "새로운 파트너십");
        Input(editor, "sponsor_name", "후원 기업", "");
        Input(editor, "city_project", "지역 프로젝트", "");
        Section(editor, "기존 후원 계약 교체");
        editor.Children.Add(Text("기존 계약을 명시적으로 선택하면 금액과 기간을 변경합니다.", 12));
        _sponsors.Margin = new Thickness(0, 0, 0, 12); editor.Children.Add(_sponsors);
        _sponsors.SelectionChanged += (_, _) => InvalidatePreview();
        Input(editor, "annual_gbp", "연간 후원금 £ · 0이면 변경 없음", "0");
        Input(editor, "years", "후원 기간 · 1–30년", "10");
        Input(editor, "krw_per_gbp", "원화 환산 표시 £1당 원 · 선택", "");
        Section(editor, "즉시 지원금");
        Input(editor, "cash_grant_gbp", "구단 현금 증가 £", "0");
        Input(editor, "transfer_budget_grant_gbp", "이적 예산 증가 £", "0");
        Input(editor, "weekly_wage_increase_gbp", "주급 예산 증가 £ / 주", "0");
        Section(editor, "도시 개발 · 빈칸은 유지");
        Input(editor, "population_band", "인구 등급 · 0–15", "");
        Input(editor, "attraction", "도시 매력도 · 0–20", "");
        editor.Children.Add(Text("인구 등급은 실제 인구수가 아닙니다. 연도별 자동 성장이나 부유도 변경은 아직 지원하지 않습니다.", 12));
        _body.Children.Add(Card(new ScrollViewer { Content = editor, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }));
        var right = new Grid(); right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.RowDefinitions.Add(new RowDefinition { Height = new GridLength(235) }); right.RowDefinitions.Add(new RowDefinition());
        var summary = new StackPanel(); summary.Children.Add(Text("적용 전 검토", 17, true));
        summary.Children.Add(Text("기업명과 기사 본문은 앱에 저장하는 가상 시나리오입니다. FM 뉴스나 회사 이름에 삽입되지 않습니다.", 12));
        _warnings.Margin = new Thickness(0, 5, 0, 12); summary.Children.Add(_warnings); right.Children.Add(summary);
        Column(_changes, "항목", nameof(ChangeRow.Label), 1.5); Column(_changes, "현재", nameof(ChangeRow.Before)); Column(_changes, "변경 후", nameof(ChangeRow.After));
        Grid.SetRow(_changes, 1); right.Children.Add(_changes);
        _narrative.Text = "왼쪽 내용을 입력한 다음 ‘변경 미리보기’를 누르세요.\n\n후원 기간은 현재 게임 날짜부터 계산됩니다. 실제 변경되는 값과 기존 계약의 날짜를 여기에서 확인할 수 있습니다.";
        _narrative.Margin = new Thickness(0, 16, 0, 0); Grid.SetRow(_narrative, 2); right.Children.Add(_narrative);
        var card = Card(right); Grid.SetColumn(card, 2); _body.Children.Add(card); RefreshButtons();
    }

    private void Section(StackPanel panel, string name) => panel.Children.Add(new TextBlock { Text = name, FontSize = 14, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 18, 0, 9), Foreground = Brush("InkBrush", "#EDF1F7") });
    private void Input(StackPanel panel, string key, string label, string initial)
    {
        panel.Children.Add(Text(label, 12));
        var input = new TextBox { Text = initial, Tag = label, MinHeight = 34, Padding = new Thickness(10, 6, 10, 6), Margin = new Thickness(0, 0, 0, 9) };
        input.TextChanged += (_, _) => InvalidatePreview(); _inputs[key] = input; panel.Children.Add(input);
    }
    private async Task LoadEventContextAsync()
    {
        if (_clubId is not uint club) { _status.Text = "먼저 구단 상세 화면에서 이벤트 스튜디오를 여세요."; return; }
        var context = await CallAsync("event_context", new { club_id = club }, _closing.Token);
        _loading = true;
        try
        {
            _live = Bool(context, "live");
            var clubName = context.TryGetProperty("club", out var c) && c.TryGetProperty("entity", out var e) ? Str(e, "name") : $"구단 {club}";
            _contextLabel.Text = $"{clubName}  ·  {Str(context, "game_date")}  ·  {(_live ? "실시간 연결" : "스냅샷 · 읽기 전용")}";
            var options = new List<SponsorOption> { new(null, "기존 후원 계약 변경 없음") };
            foreach (var sponsor in Array(context, "sponsors"))
                options.Add(new(Int(sponsor, "slot"), $"계약 {Int(sponsor, "slot") + 1} · 유형 {Int(sponsor, "kind")} · 연 £{Number(sponsor, "annual_gbp"):N0}\n총 £{Number(sponsor, "total_gbp"):N0} · {Str(sponsor, "start")} ~ {Str(sponsor, "end")}"));
            _sponsors.ItemsSource = options; _sponsors.SelectedIndex = 0;
            if (context.TryGetProperty("draft_defaults", out var defaults))
                foreach (var key in new[] { "title", "sponsor_name", "city_project", "years" })
                    if (defaults.TryGetProperty(key, out var value)) _inputs[key].Text = value.ToString();
            _warnings.Text = string.Join("\n", Array(context, "warnings").Select(x => x.ToString()));
            _status.Text = _live ? "미리보기는 값을 쓰지 않습니다. 검토 후 적용하면 백업과 원래 값 검증을 거칩니다." : "스냅샷 모드: 미리보기와 로컬 시나리오 저장을 사용할 수 있습니다.";
        }
        finally { _loading = false; InvalidatePreview(); }
    }

    private Dictionary<string, object?> ReadDraft()
    {
        var draft = new Dictionary<string, object?>();
        foreach (var key in new[] { "title", "sponsor_name", "city_project" }) draft[key] = _inputs[key].Text.Trim();
        if (string.IsNullOrWhiteSpace((string?)draft["title"])) throw new InvalidOperationException("이벤트 제목을 입력하세요.");
        foreach (var key in new[] { "annual_gbp", "cash_grant_gbp", "transfer_budget_grant_gbp", "weekly_wage_increase_gbp" }) draft[key] = ReadInteger(key, 0, 2_000_000_000);
        draft["years"] = ReadInteger("years", 1, 30);
        draft["sponsor_slot"] = (_sponsors.SelectedItem as SponsorOption)?.Slot;
        if ((long)draft["annual_gbp"]! > 0 && draft["sponsor_slot"] is null) throw new InvalidOperationException("후원금을 변경할 기존 계약을 선택하세요.");
        foreach (var key in new[] { "population_band", "attraction" }) draft[key] = string.IsNullOrWhiteSpace(_inputs[key].Text) ? null : ReadInteger(key, 0, key == "population_band" ? 15 : 20);
        var rate = _inputs["krw_per_gbp"].Text.Trim();
        if (rate.Length == 0) draft["krw_per_gbp"] = null;
        else if (double.TryParse(rate, NumberStyles.Number, CultureInfo.InvariantCulture, out var amount) && amount > 0 && amount <= 1_000_000) draft["krw_per_gbp"] = amount;
        else throw new InvalidOperationException("환율은 0보다 크고 1,000,000 이하인 숫자로 입력하세요.");
        return draft;
    }
    private long ReadInteger(string key, long min, long max)
    {
        if (long.TryParse(_inputs[key].Text, NumberStyles.Integer | NumberStyles.AllowThousands, CultureInfo.InvariantCulture, out var number) && number >= min && number <= max) return number;
        throw new InvalidOperationException($"{_inputs[key].Tag}: {min:N0}–{max:N0} 범위의 정수로 입력하세요.");
    }
    private async Task PreviewEventAsync()
    {
        var draft = ReadDraft();
        var preview = await CallAsync("event_preview", new { club_id = _clubId, draft }, _closing.Token);
        _lastPreview = preview.Clone(); _token = Str(preview, "token"); _canApply = Bool(preview, "can_apply");
        _changes.ItemsSource = Array(preview, "deltas").Select(x => new ChangeRow(Str(x, "section"), Str(x, "label"), Str(x, "before"), Str(x, "after"))).ToList();
        _narrative.Text = Str(preview, "narrative"); _warnings.Text = string.Join("\n", Array(preview, "warnings").Select(x => x.ToString()));
        _status.Text = $"미리보기 완료 · {Array(preview, "deltas").Count()}개 변경 항목 · {(_canApply ? "적용 가능" : "읽기 전용 또는 적용할 변경 없음")}";
    }
    private async Task SaveEventAsync()
    {
        RequireToken(); var result = await CallAsync("event_save", new { token = _token }, _closing.Token);
        _status.Text = $"시나리오 저장 완료: {Str(result, "path")}\n기사 파일: {Str(result, "narrative_path")}";
    }
    private async Task ApplyEventAsync()
    {
        RequireToken();
        if (!_canApply || !_live) throw new InvalidOperationException("실시간 연결에서 검토한 변경만 적용할 수 있습니다.");
        var changes = _changes.ItemsSource as IEnumerable<ChangeRow> ?? Enumerable.Empty<ChangeRow>();
        var review = string.Join("\n", changes.Select(x => $"{x.Label}: {x.Before} → {x.After}"));
        if (AppDialog.Show(this, $"검토한 값을 FM에 적용합니다.\n\n{review}\n\n백업 후 원래 값이 일치하는지 확인합니다. 게임 저장은 별도로 해야 합니다.", "이벤트 변경 적용", MessageBoxButton.OKCancel, MessageBoxImage.Question) != MessageBoxResult.OK) return;
        var token = _token; InvalidatePreview();
        var receipt = await CallAsync("event_apply", new { token }, _closing.Token);
        _status.Text = $"적용 결과: {Str(receipt, "status")} · 기록 {Str(receipt, "id")}\nFM 자체 저장은 수행하지 않았습니다. 변경 기록에서 되돌릴 수 있습니다.";
        _warnings.Text = "적용한 뒤 다시 수정하려면 창을 다시 열어 현재 값을 불러오세요.";
    }
    private void RequireToken()
    {
        if (string.IsNullOrEmpty(_token)) throw new InvalidOperationException("먼저 변경 미리보기를 실행하세요.");
    }

    /// <summary>Offline UI acceptance path. Never sends apply/undo or accesses live FM.</summary>
    public async Task<object> QaPreviewAsync(decimal cashGrant = 1000)
    {
        if (_mode != "events") throw new InvalidOperationException("이벤트 창에서만 실행할 수 있습니다.");
        var deadline = DateTime.UtcNow.AddSeconds(30);
        while (_busy && DateTime.UtcNow < deadline) await Task.Delay(50, _closing.Token);
        if (_busy) throw new TimeoutException("이벤트 컨텍스트 로딩 시간이 초과되었습니다.");
        if (_live) throw new InvalidOperationException("오프라인 QA는 실시간 세션에서 실행할 수 없습니다.");
        _inputs["cash_grant_gbp"].Text = cashGrant.ToString(CultureInfo.InvariantCulture);
        await PreviewEventAsync(); RefreshButtons();
        if (string.IsNullOrEmpty(_token) || !_lastPreview.HasValue) throw new InvalidOperationException("미리보기 토큰이 없습니다.");
        var deltas = Array(_lastPreview.Value, "deltas").ToList();
        var readOnly = !_canApply && _applyButton is { IsEnabled: false };
        _inputs["cash_grant_gbp"].Text = (cashGrant + 1).ToString(CultureInfo.InvariantCulture);
        var invalidates = _token is null && _saveButton is { IsEnabled: false };
        _inputs["cash_grant_gbp"].Text = "2000000001";
        var rangeRejected = false;
        try { ReadDraft(); } catch (InvalidOperationException) { rangeRejected = true; }
        _inputs["cash_grant_gbp"].Text = cashGrant.ToString(CultureInfo.InvariantCulture);
        await PreviewEventAsync(); RefreshButtons();
        if (deltas.Count == 0 || !readOnly || !invalidates || !rangeRejected) throw new InvalidOperationException("이벤트 미리보기 QA 검증 실패");
        return new
        {
            preview_token = !string.IsNullOrEmpty(_token), delta_count = deltas.Count, apply_disabled_offline = readOnly,
            edits_invalidate_preview = invalidates, rejects_out_of_range_money = rangeRejected,
            narrative = !string.IsNullOrWhiteSpace(_narrative.Text), game_accessed = false, game_modified = false
        };
    }

    public async Task<object> QaHistoryAsync()
    {
        if (_mode != "history" || _live) throw new InvalidOperationException("오프라인 기록 창에서만 실행할 수 있습니다.");
        var deadline = DateTime.UtcNow.AddSeconds(30);
        while (_busy && DateTime.UtcNow < deadline) await Task.Delay(50, _closing.Token);
        if (_busy) throw new TimeoutException("변경 기록 로딩 시간이 초과되었습니다.");
        await LoadHistoryAsync(); RefreshButtons();
        return new { scalar_and_event_history_loaded = true, relation_history_loaded = true, receipt_count = _receipts.Count, undo_disabled_offline = _undoButton is { IsEnabled: false }, game_accessed = false, game_modified = false };
    }
    public async Task<object> QaAbilityVisibilityAsync()
    {
        if (_mode != "history" || _live || _history == null) throw new InvalidOperationException("능력치 표시 QA는 스냅샷 기록 창에서 실행합니다.");
        var deadline = DateTime.UtcNow.AddSeconds(30);
        while (_busy && DateTime.UtcNow < deadline) await Task.Delay(50, _closing.Token);
        if (_busy) throw new TimeoutException("변경 기록 로딩 시간이 초과되었습니다.");
        bool originalCa = AbilityVisibility.Current.HideCa, originalPa = AbilityVisibility.Current.HidePa;
        var originalItems = _history.ItemsSource; object? originalSelection = _history.SelectedItem;
        using var sample = JsonDocument.Parse("{\"changes\":[{\"key\":\"ca\",\"label\":\"CA\",\"before\":73,\"after\":74},{\"key\":\"pa\",\"label\":\"PA\",\"before\":182,\"after\":183}]}");
        var row = new ReceiptRow("ability-visibility-qa", "", "능력치", "QA", "applied", false, false, sample.RootElement.Clone());
        var reports = new List<object>();
        try
        {
            _history.ItemsSource = new[] { row }; _history.SelectedItem = row;
            foreach (var (hideCa, hidePa) in new[] { (true, false), (false, true), (true, true), (false, false) })
            {
                AbilityVisibility.Current.Set(hideCa, hidePa);
                await Dispatcher.InvokeAsync(UpdateLayout);
                string expected = $"CA: {(hideCa ? "숨김" : "73")} → {(hideCa ? "숨김" : "74")}\nPA: {(hidePa ? "숨김" : "182")} → {(hidePa ? "숨김" : "183")}";
                if (_narrative.Text != expected || _undoButton?.IsEnabled == true) throw new InvalidOperationException("기록의 이전/이후 수치 숨김 또는 스냅샷 보호가 올바르지 않습니다.");
                reports.Add(new { hideCa, hidePa, beforeAndAfterMasked = true, offlineUndoDisabled = true });
            }
            return new { success = true, gameWrites = false, reports };
        }
        finally { _history.ItemsSource = originalItems; _history.SelectedItem = originalSelection; AbilityVisibility.Current.Set(originalCa, originalPa); if (originalSelection != null) ShowReceipt(); else _narrative.Text = "기록을 선택하면 적용한 값을 확인할 수 있습니다."; }
    }
    private void InvalidatePreview()
    {
        if (_loading) return;
        _token = null; _canApply = false; _lastPreview = null;
        RefreshButtons();
    }

    private void BuildHistory()
    {
        Action("새로 고침", LoadHistoryAsync); _undoButton = Action("선택 기록 되돌리기", UndoAsync);
        _body.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        _body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        _body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(210) });
        _body.Children.Add(Text("현재 값이 적용 후 값과 일치하고 같은 세션일 때만 되돌릴 수 있습니다. 다른 게임을 로드한 뒤의 기록은 적용되지 않습니다.", 13));
        _history = Table(); Column(_history, "시간", nameof(ReceiptRow.Time), 1.2); Column(_history, "유형", nameof(ReceiptRow.Type), .6);
        Column(_history, "대상", nameof(ReceiptRow.Target), 1.2); Column(_history, "결과", nameof(ReceiptRow.Status)); Column(_history, "기록 ID", nameof(ReceiptRow.Id), 1.4);
        _history.Margin = new Thickness(0, 15, 0, 15); _history.SelectionChanged += (_, _) => ShowReceipt(); Grid.SetRow(_history, 1); _body.Children.Add(_history);
        _narrative.Text = "기록을 선택하면 적용한 값을 확인할 수 있습니다."; Grid.SetRow(_narrative, 2); _body.Children.Add(_narrative); RefreshButtons();
    }
    private async Task LoadHistoryAsync()
    {
        _receipts.Clear();
        foreach (var command in new[] { "history", "event_history", "relation_history" })
        {
            var result = await CallAsync(command, null, _closing.Token);
            if (result.ValueKind != JsonValueKind.Array) continue;
            foreach (var receipt in result.EnumerateArray())
            {
                var eventRecord = command == "event_history";
                string target = "";
                if (receipt.TryGetProperty("entity", out var entity)) target = Str(entity, "name");
                if (eventRecord && receipt.TryGetProperty("plan", out var plan))
                {
                    if (plan.TryGetProperty("draft", out var draft)) target = Str(draft, "title");
                }
                var rawTime = Str(receipt, "time"); var time = rawTime;
                if (long.TryParse(rawTime, out var ms) && ms > 0 && ms < 253402300799999) time = DateTimeOffset.FromUnixTimeMilliseconds(ms).ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss");
                bool relationRecord=command=="relation_history";
                _receipts.Add(new(Str(receipt, "id"), time, eventRecord ? "이벤트" : relationRecord ? "인물 관계" : "능력치", target, Str(receipt, "status"), eventRecord, relationRecord, receipt.Clone()));
            }
        }
        if (_history is not null) _history.ItemsSource = _receipts.OrderByDescending(x => x.Time).ToList();
        _status.Text = $"총 {_receipts.Count}개 기록 · {(_live ? "실시간 세션에서 되돌리기 가능" : "스냅샷 모드 · 읽기 전용")}";
    }
    private void ShowReceipt()
    {
        if (_history?.SelectedItem is not ReceiptRow row) return;
        var r = row.Raw;
        if (row.IsEvent && r.TryGetProperty("plan", out var plan))
        {
            _narrative.Text = string.Join("\n", Array(plan, "deltas").Select(x => $"{Str(x, "label")}: {Str(x, "before")} → {Str(x, "after")}")) + "\n\n" + Str(plan, "narrative");
        }
        else if(row.IsRelation) _narrative.Text=Str(r,"summary")+"\n\n"+Str(r,"before")+"  →  "+Str(r,"after");
        else _narrative.Text = string.Join("\n", Array(r, "changes").Select(x => $"{Str(x, "label")}: {AbilityVisibility.Current.Format(Str(x, "key"), Str(x, "before"))} → {AbilityVisibility.Current.Format(Str(x, "key"), Str(x, "after"))}"));
        RefreshButtons();
    }
    private void AbilityVisibilityChanged(object? sender, EventArgs args)
    {
        if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(() => AbilityVisibilityChanged(sender, args)); return; }
        if (_mode == "history") ShowReceipt();
    }
    private static bool HasHiddenChanges(ReceiptRow row) => !row.IsEvent && !row.IsRelation && Array(row.Raw, "changes").Any(change => AbilityVisibility.Current.IsHidden(Str(change, "key")));
    private async Task UndoAsync()
    {
        if (!_live || _history?.SelectedItem is not ReceiptRow row || HasHiddenChanges(row) || (row.IsRelation && !Bool(row.Raw,"can_undo"))) return;
        if (AppDialog.Show(this, $"{row.Target}\n기록 {row.Id}\n\n이 기록에 포함된 변경을 되돌릴까요? 현재 값이 이후에 바뀌었다면 중단됩니다.", "변경 되돌리기", MessageBoxButton.OKCancel, MessageBoxImage.Question) != MessageBoxResult.OK) return;
        await UndoReviewedAsync(row);
    }
    private async Task UndoReviewedAsync(ReceiptRow row)
    {
        if (_connectionInvalidated || !_live || HasHiddenChanges(row) || !ReferenceEquals(_history?.SelectedItem, row) || (row.IsRelation && !Bool(row.Raw,"can_undo"))) return;
        if (BeforeUndo != null && !await BeforeUndo()) return;
        try
        {
            var result = await CallAsync(row.IsEvent ? "event_undo" : row.IsRelation ? "relation_undo" : "undo", new { receipt_id = row.Id });
            await LoadHistoryAsync(); _status.Text = $"되돌리기 결과: {Str(result, "status")} · {Str(result, "id")}";
        }
        finally { if (AfterUndo != null) await AfterUndo(); }
    }

    private void BuildOverlay(EntityRecord? staff)
    {
        _overlayStaff = staff;
        var anchor = new ComboBox { ItemsSource = new[] { "오른쪽 위", "왼쪽 위", "오른쪽 아래", "왼쪽 아래" }, SelectedIndex = 0, MinHeight = 38, MaxWidth = 290, HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 0, 0, 18) };
        var group = new ComboBox { ItemsSource = new[] { "훈련", "육성 · 관리", "영입 · 지원" }, SelectedIndex = 0, MinHeight = 38, MaxWidth = 290, HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 0, 0, 18) };
        _overlayStartButton = Action("오버레이 켜기", async () =>
        {
            if (staff is null) throw new InvalidOperationException("먼저 코치 상세 화면에서 스태프를 선택하세요.");
            _overlay?.Close(); _overlay = null;
            _overlay = await CoachOverlayWindow.StartAsync(_bridge, staff, _closing.Token);
            if (_closing.IsCancellationRequested) { _overlay.Close(); _overlay = null; return; }
            _overlay.SetAnchor(anchor.SelectedItem?.ToString() ?? "오른쪽 위");
            _overlay.SetScoreGroup(group.SelectedItem?.ToString() ?? "훈련");
            _overlay.StatusChanged += message => _status.Text = message;
            _status.Text = $"{staff.DisplayName} 고정 완료 · FM을 직접 선택하면 오버레이가 표시됩니다.";
        });
        _overlayStopButton = Action("끄기", () => { _overlay?.Close(); _overlay = null; _status.Text = "오버레이를 껐습니다."; return Task.CompletedTask; });
        anchor.SelectionChanged += (_, _) => _overlay?.SetAnchor(anchor.SelectedItem?.ToString() ?? "오른쪽 위");
        group.SelectionChanged += (_, _) => _overlay?.SetScoreGroup(group.SelectedItem?.ToString() ?? "훈련");
        var content = new StackPanel { MaxWidth = 680, HorizontalAlignment = HorizontalAlignment.Left };
        content.Children.Add(Text(staff?.DisplayName ?? "선택된 코치 없음", 22, true));
        content.Children.Add(Text(staff is null ? "코치 상세 화면에서 오버레이를 여세요." : $"{staff.Club} · {staff.Role} · UID {staff.Id}", 14));
        content.Children.Add(Text("선택한 코치의 능력치 23개와 역할별 점수를 FM 위에 표시합니다. FM이 전경일 때만 나타나고, 클릭은 게임으로 전달됩니다.", 15));
        Section(content, "표시 위치"); content.Children.Add(anchor);
        Section(content, "직무 점수 그룹"); content.Children.Add(group);
        content.Children.Add(Text("현재 FM에서 보고 있는 프로필을 자동으로 판별하지 않습니다. 다른 코치를 보려면 이 창을 닫고 새 코치를 선택하세요.", 13));
        content.Children.Add(Text("2초마다 읽기 전용으로 갱신합니다. 5초 이상 정보가 갱신되지 않거나 FM 연결이 바뀌면 숨겨집니다. 이 설정 창을 닫으면 오버레이도 종료됩니다.", 13));
        _body.Children.Add(Card(content)); _status.Text = _live ? "오버레이는 기본적으로 꺼져 있습니다." : "스냅샷 모드에서는 오버레이를 켤 수 없습니다.";
        RefreshButtons();
    }
    private async Task RunAsync(Func<Task> callback)
    {
        if (_busy || _connectionInvalidated) return;
        _busy = true; _body.IsEnabled = false; RefreshButtons();
        try { await callback(); }
        catch (OperationCanceledException) { }
        catch (Exception ex) { _status.Text = $"작업을 완료하지 못했습니다: {ex.Message}"; }
        finally { _busy = false; _body.IsEnabled = !_connectionInvalidated; if (!_closing.IsCancellationRequested) RefreshButtons(); }
    }
    private void RefreshButtons()
    {
        if (_connectionInvalidated) { foreach (var child in _actions.Children.OfType<Button>()) child.IsEnabled = false; return; }
        foreach (var child in _actions.Children.OfType<Button>()) child.IsEnabled = !_busy;
        if (_previewButton is not null) _previewButton.IsEnabled = !_busy && _clubId.HasValue;
        if (_saveButton is not null) _saveButton.IsEnabled = !_busy && !string.IsNullOrEmpty(_token);
        if (_applyButton is not null) _applyButton.IsEnabled = !_busy && _live && _canApply && !string.IsNullOrEmpty(_token);
        if (_undoButton is not null) _undoButton.IsEnabled = !_busy && _live && _history?.SelectedItem is ReceiptRow row && !HasHiddenChanges(row) && row.Status.StartsWith("applied", StringComparison.OrdinalIgnoreCase) && (!row.IsRelation || Bool(row.Raw,"can_undo"));
        if (_overlayStartButton is not null) _overlayStartButton.IsEnabled = !_busy && _live && _overlayStaff is not null;
        if (_overlayStopButton is not null) _overlayStopButton.IsEnabled = !_busy && _overlay is not null;
    }
    private static string Str(JsonElement e, string name) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind != JsonValueKind.Null ? v.ToString() : "";
    private static bool Bool(JsonElement e, string name) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
    private static int Int(JsonElement e, string name) => e.TryGetProperty(name, out var v) && v.TryGetInt32(out var n) ? n : 0;
    private static double Number(JsonElement e, string name) => e.TryGetProperty(name, out var v) && v.TryGetDouble(out var n) ? n : 0;
    private static IEnumerable<JsonElement> Array(JsonElement e, string name) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var a) && a.ValueKind == JsonValueKind.Array ? a.EnumerateArray() : Enumerable.Empty<JsonElement>();
    private sealed record SponsorOption(int? Slot, string Label);
    private sealed record ChangeRow(string Section, string Label, string Before, string After);
    private sealed record ReceiptRow(string Id, string Time, string Type, string Target, string Status, bool IsEvent, bool IsRelation, JsonElement Raw)
    {
        public override string ToString() => $"{Time} · {Type} · {Target} · {Status}";
    }
}
