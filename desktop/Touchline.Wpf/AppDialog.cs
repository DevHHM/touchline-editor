using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace Touchline.Wpf;

/// <summary>Application prompts share the embedded font and a safe confirmation default.</summary>
public static class AppDialog
{
 public static MessageBoxResult Show(string message, string title, MessageBoxButton buttons = MessageBoxButton.OK, MessageBoxImage icon = MessageBoxImage.None)
  => Show(null, message, title, buttons, icon);

 public static MessageBoxResult Show(Window? owner, string message, string title, MessageBoxButton buttons = MessageBoxButton.OK, MessageBoxImage icon = MessageBoxImage.None)
 {
  owner = owner is { IsVisible: true } ? owner : Application.Current?.Windows.OfType<Window>().FirstOrDefault(window => window.IsActive);
  var dialog = new PromptWindow(owner, message, title, buttons, icon);
  dialog.ShowDialog();
  return dialog.Result;
 }

 private static MessageBoxResult DismissResult(MessageBoxButton buttons) => buttons switch
 {
  MessageBoxButton.OK => MessageBoxResult.OK,
  MessageBoxButton.OKCancel or MessageBoxButton.YesNoCancel => MessageBoxResult.Cancel,
  MessageBoxButton.YesNo => MessageBoxResult.No,
  _ => throw new ArgumentOutOfRangeException(nameof(buttons))
 };

 private static (string Label, MessageBoxResult Result)[] Choices(MessageBoxButton buttons) => buttons switch
 {
  MessageBoxButton.OK => [("확인", MessageBoxResult.OK)],
  MessageBoxButton.OKCancel => [("확인", MessageBoxResult.OK), ("취소", MessageBoxResult.Cancel)],
  MessageBoxButton.YesNo => [("예", MessageBoxResult.Yes), ("아니요", MessageBoxResult.No)],
  MessageBoxButton.YesNoCancel => [("예", MessageBoxResult.Yes), ("아니요", MessageBoxResult.No), ("취소", MessageBoxResult.Cancel)],
  _ => throw new ArgumentOutOfRangeException(nameof(buttons))
 };

 private sealed class PromptWindow : Window
 {
  public MessageBoxResult Result { get; private set; }
  public Border Surface { get; }
  public IReadOnlyList<Button> Choices { get; }

  public PromptWindow(Window? owner, string message, string title, MessageBoxButton buttons, MessageBoxImage icon)
  {
   Result = DismissResult(buttons);
   var dismissResult = Result;
   Title = title;
   Owner = owner;
   ShowInTaskbar = owner == null;
   WindowStyle = WindowStyle.None;
   ResizeMode = ResizeMode.NoResize;
   WindowStartupLocation = owner == null ? WindowStartupLocation.CenterScreen : WindowStartupLocation.CenterOwner;
   Width = Math.Min(620, Math.Max(320, SystemParameters.WorkArea.Width - 48));
   SizeToContent = SizeToContent.Height;
   MaxHeight = Math.Max(260, SystemParameters.WorkArea.Height - 48);
   Background = ColorBrush("#181E29");
   Foreground = ColorBrush("#EBF0F8");
   FontSize = 13;
   Typography.Initialize(this);

   var panel = new DockPanel { LastChildFill = true };
   Surface = new Border { Background = Background, BorderBrush = ColorBrush("#3A4D5B"), BorderThickness = new Thickness(1), Padding = new Thickness(24), Child = panel };
   Content = Surface;

   var header = new Grid { Margin = new Thickness(0, 0, 0, 20) };
   header.ColumnDefinitions.Add(new ColumnDefinition());
   header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
   header.Children.Add(new TextBlock { Text = title, FontSize = 20, FontWeight = FontWeights.Bold, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 0, 16, 0), VerticalAlignment = VerticalAlignment.Center });
   var close = new Button { Content = "×", Width = 34, Height = 32, Padding = new Thickness(0), FontSize = 20, ToolTip = "닫기", Background = Brushes.Transparent, BorderThickness = new Thickness(0), Focusable = false };
   close.Click += (_, _) => Close();
   Grid.SetColumn(close, 1);
   header.Children.Add(close);
   header.MouseLeftButtonDown += (_, args) => { if (args.OriginalSource is TextBlock && args.LeftButton == MouseButtonState.Pressed) DragMove(); };
   DockPanel.SetDock(header, Dock.Top);
   panel.Children.Add(header);

   if (icon != MessageBoxImage.None)
   {
    var label = icon == MessageBoxImage.Error ? "오류" : icon == MessageBoxImage.Warning ? "확인해 주세요" : icon == MessageBoxImage.Question ? "진행 전 확인" : "안내";
    var hint = new TextBlock { Text = label, FontSize = 12, FontWeight = FontWeights.SemiBold, Foreground = ColorBrush(icon == MessageBoxImage.Error ? "#F5A0A0" : icon == MessageBoxImage.Warning ? "#E9C882" : "#75DFC4"), Margin = new Thickness(0, 0, 0, 10) };
    DockPanel.SetDock(hint, Dock.Top);
    panel.Children.Add(hint);
   }

   var footer = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 24, 0, 0) };
   DockPanel.SetDock(footer, Dock.Bottom);
   panel.Children.Add(footer);
   var choiceButtons = new List<Button>();
   foreach (var choice in AppDialog.Choices(buttons))
   {
    bool safeDefault = choice.Result == dismissResult;
    var button = new Button { Content = choice.Label, Tag = choice.Result, MinWidth = 96, Padding = new Thickness(18, 9, 18, 9), Margin = new Thickness(8, 0, 0, 0), IsDefault = safeDefault, IsCancel = safeDefault, FontWeight = safeDefault ? FontWeights.SemiBold : FontWeights.Medium };
    if (safeDefault) button.BorderBrush = ColorBrush("#75DFC4");
    button.Click += (_, _) => { Result = choice.Result; Close(); };
    footer.Children.Add(button);
    choiceButtons.Add(button);
   }
   Choices = choiceButtons;
   var body = new TextBox { Text = message, IsReadOnly = true, IsReadOnlyCaretVisible = false, AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, Background = Brushes.Transparent, Foreground = Foreground, BorderThickness = new Thickness(0), Padding = new Thickness(0), FontSize = 14, MinHeight = 36, MaxHeight = Math.Max(80, Math.Min(600, MaxHeight - 200)) };
   panel.Children.Add(body);
   ContentRendered += (_, _) => choiceButtons.Single(button => button.IsDefault).Focus();
   PreviewKeyDown += (_, args) => { if (args.Key == Key.Escape) { args.Handled = true; Result = dismissResult; Close(); } };
  }
 }

 private static SolidColorBrush ColorBrush(string value) => new((Color)ColorConverter.ConvertFromString(value));

 /// <summary>Renders the actual prompt content without showing a modal or executing an action.</summary>
 public static void RenderPreview(string path)
 {
  var reports = new List<object>();
  foreach (var buttons in new[] { MessageBoxButton.OK, MessageBoxButton.OKCancel, MessageBoxButton.YesNo, MessageBoxButton.YesNoCancel })
  {
   var dialog = new PromptWindow(null, "라스무스 호일룬 · Rasmus Højlund\n현재 능력 156 → 157\n\n표시한 변경 내용을 확인한 뒤 진행하세요.", "변경 검토", buttons, MessageBoxImage.Question);
   try
   {
   var defaults = dialog.Choices.Where(button => button.IsDefault).ToArray();
   var cancels = dialog.Choices.Where(button => button.IsCancel).ToArray();
   bool valid = defaults.Length == 1 && cancels.Length == 1 && Equals(defaults[0].Tag, DismissResult(buttons)) && Equals(cancels[0].Tag, DismissResult(buttons)) && dialog.Result == DismissResult(buttons);
   reports.Add(new { buttons = buttons.ToString(), defaultResult = defaults.Single().Tag.ToString(), cancelResult = cancels.Single().Tag.ToString(), closeResult = dialog.Result.ToString(), family = dialog.FontFamily.Source, valid });
   if (!valid) throw new InvalidOperationException("Dialog confirmation defaults must dismiss safely.");
   if (buttons == MessageBoxButton.YesNoCancel)
   {
    var surface = dialog.Surface;
    surface.Measure(new Size(dialog.Width, double.PositiveInfinity));
    surface.Arrange(new Rect(0, 0, dialog.Width, surface.DesiredSize.Height));
    surface.UpdateLayout();
    var bitmap = new RenderTargetBitmap((int)Math.Ceiling(surface.ActualWidth), (int)Math.Ceiling(surface.ActualHeight), 96, 96, PixelFormats.Pbgra32);
    bitmap.Render(surface);
    Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
    var encoder = new PngBitmapEncoder();
    encoder.Frames.Add(BitmapFrame.Create(bitmap));
    using var stream = File.Create(path);
    encoder.Save(stream);
   }
   }
   finally { dialog.Close(); }
  }
  File.WriteAllText(Path.ChangeExtension(path, ".json"), JsonSerializer.Serialize(new { success = true, prompts = reports }, new JsonSerializerOptions { WriteIndented = true }));
 }
}
