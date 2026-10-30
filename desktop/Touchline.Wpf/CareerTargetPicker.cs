using System.Text.Json;
using System.Text.Json.Serialization;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

public sealed class CareerTargetOption
{
    [JsonPropertyName("object_type")] public int ObjectType { get; set; }
    [JsonPropertyName("uid")] public uint Uid { get; set; }
    [JsonPropertyName("name")] public string Name { get; set; } = "";
}

/// <summary>Read-only, typed native catalog search. No pointers cross the bridge.</summary>
internal sealed class CareerTargetPicker : Window
{
    private readonly BridgeClient _bridge;
    private readonly int _type;
    private readonly TextBox _query = new() { MinHeight = 34 };
    private readonly TextBlock _status = new() { TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
    private readonly DataGrid _grid = new() { AutoGenerateColumns = false, IsReadOnly = true, CanUserAddRows = false, SelectionMode = DataGridSelectionMode.Single, HeadersVisibility = DataGridHeadersVisibility.Column };
    private readonly Button _select = new() { Content = "선택", MinWidth = 84, IsEnabled = false, Margin = new Thickness(8, 0, 0, 0) };
    private readonly DispatcherTimer _debounce = new() { Interval = TimeSpan.FromMilliseconds(300) };
    private readonly CancellationTokenSource _lifetime = new();
    private int _version;
    private bool _closed;
    public CareerTargetOption? Result { get; private set; }

    public CareerTargetPicker(BridgeClient bridge, int type)
    {
        _bridge = bridge; _type = type;
        Title = (type switch { 3 => "구단", 5 => "대륙", 9 => "국가", 25 => "대회", _ => "대상" }) + " 선택";
        Width = 800; Height = 580; MinWidth = 600; MinHeight = 420;
        WindowStartupLocation = WindowStartupLocation.CenterOwner; ShowInTaskbar = false;
        Background = (Brush)FindResource("BaseBrush"); Foreground = (Brush)FindResource("InkBrush"); Typography.Initialize(this);
        var root = new DockPanel { Margin = new Thickness(20) }; Content = root;
        var top = new StackPanel { Margin = new Thickness(0, 0, 0, 12) };
        top.Children.Add(new TextBlock { Text = "이름 또는 UID 검색", Margin = new Thickness(0, 0, 0, 6) }); top.Children.Add(_query);
        DockPanel.SetDock(top, Dock.Top); root.Children.Add(top);
        var bottom = new DockPanel { Margin = new Thickness(0, 12, 0, 0) };
        var buttons = new StackPanel { Orientation = Orientation.Horizontal };
        var cancel = new Button { Content = "취소", IsCancel = true, MinWidth = 84 };
        cancel.Click += (_, _) => Close(); buttons.Children.Add(cancel); buttons.Children.Add(_select);
        DockPanel.SetDock(buttons, Dock.Right); bottom.Children.Add(buttons); bottom.Children.Add(_status);
        DockPanel.SetDock(bottom, Dock.Bottom); root.Children.Add(bottom);
        _grid.Columns.Add(new DataGridTextColumn { Header = "이름", Binding = new Binding(nameof(CareerTargetOption.Name)), Width = new DataGridLength(1, DataGridLengthUnitType.Star) });
        _grid.Columns.Add(new DataGridTextColumn { Header = "UID", Binding = new Binding(nameof(CareerTargetOption.Uid)), Width = 120 }); root.Children.Add(_grid);
        _query.TextChanged += (_, _) => { ++_version; _grid.ItemsSource = null; _select.IsEnabled = false; _debounce.Stop(); _debounce.Start(); };
        _debounce.Tick += async (_, _) => { _debounce.Stop(); await SearchAsync(); };
        _query.KeyDown += async (_, e) => { if (e.Key == Key.Enter) { e.Handled = true; _debounce.Stop(); await SearchAsync(); } };
        _grid.SelectionChanged += (_, _) => _select.IsEnabled = !_closed && _grid.SelectedItem is CareerTargetOption;
        _grid.MouseDoubleClick += (_, _) => Choose(); _select.Click += (_, _) => Choose();
        Loaded += async (_, _) => { _query.Focus(); await SearchAsync(); };
        Closed += (_, _) => { _closed = true; ++_version; _debounce.Stop(); _lifetime.Cancel(); };
    }
    private async Task SearchAsync()
    {
        if (_closed) return;
        int version = ++_version; _select.IsEnabled = false; _status.Text = "검색 중…";
        try
        {
            var result = await _bridge.CallAsync("career_targets", new { object_type = _type, query = _query.Text.Trim(), limit = 200 }, _lifetime.Token);
            if (_closed || version != _version) return;
            var rows = JsonSerializer.Deserialize<List<CareerTargetOption>>(result.GetProperty("items"), JsonDefaults.Options) ?? [];
            _grid.ItemsSource = rows;
            _status.Text = rows.Count == 200 ? "최대 200개 표시 · 검색어로 범위를 좁혀 주세요." : $"{rows.Count:N0}개 검색됨";
        }
        catch (OperationCanceledException) when (_closed) { }
        catch (Exception e) { if (!_closed && version == _version) _status.Text = e.Message; }
    }
    private void Choose()
    {
        if (_closed || !_select.IsEnabled || _grid.SelectedItem is not CareerTargetOption row) return;
        Result = row; DialogResult = true;
    }
    internal async Task QaAsync(uint uid, string path)
    {
        _query.Text = uid.ToString(System.Globalization.CultureInfo.InvariantCulture); _debounce.Stop(); await SearchAsync();
        _grid.SelectedItem = _grid.Items.OfType<CareerTargetOption>().FirstOrDefault(row => row.Uid == uid && row.ObjectType == _type)
            ?? throw new InvalidOperationException("형식별 대상 검색 결과에 QA 대상이 없습니다.");
        await Dispatcher.InvokeAsync(UpdateLayout, DispatcherPriority.Render);
        var bitmap = new RenderTargetBitmap((int)ActualWidth, (int)ActualHeight, 96, 96, PixelFormats.Pbgra32); bitmap.Render(this);
        var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using (var file = File.Create(path)) encoder.Save(file);
        Choose();
    }
}
