using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private double _automaticListWidth = 294;
 private static string TextRenderingSettingsPath => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "TouchlineEditor", "text-rendering.json");

 private void LoadTextRenderingPreference()
 {
  var mode = TextRenderingMode.Grayscale;
  try
  {
   if (File.Exists(TextRenderingSettingsPath) && Enum.TryParse<TextRenderingMode>(JsonSerializer.Deserialize<string>(File.ReadAllText(TextRenderingSettingsPath)), out var saved) && saved is TextRenderingMode.Grayscale or TextRenderingMode.ClearType)
    mode = saved;
  }
  catch (Exception error) when (error is IOException or JsonException or UnauthorizedAccessException) { App.Log(error); }
  SetTextRenderingMode(mode);
 }

 private void SetTextRenderingMode(TextRenderingMode mode)
 {
  Application.Current.Resources["AppTextRenderingMode"] = mode;
  GrayscaleTextOption.IsChecked = mode == TextRenderingMode.Grayscale;
  ClearTypeTextOption.IsChecked = mode == TextRenderingMode.ClearType;
 }

 private void TextRenderingMenu_Click(object sender, RoutedEventArgs args)
 {
  if (sender is Button { ContextMenu: { } menu } button)
  {
   menu.PlacementTarget = button;
   menu.IsOpen = true;
  }
 }

 private void TextRenderingOption_Click(object sender, RoutedEventArgs args)
 {
  if (sender is not MenuItem item || !Enum.TryParse<TextRenderingMode>(item.Tag?.ToString(), out var mode)) return;
  SetTextRenderingMode(mode);
  try
  {
   Directory.CreateDirectory(Path.GetDirectoryName(TextRenderingSettingsPath)!);
   File.WriteAllText(TextRenderingSettingsPath, JsonSerializer.Serialize(mode.ToString()));
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException) { App.Log(error); }
  Status($"Pretendard · {(mode == TextRenderingMode.Grayscale ? "회색조" : "ClearType")} 글자 표시");
 }

 private void InitializeResponsiveLayout()
 {
  Typography.Initialize(this);
  LoadTextRenderingPreference();
  SizeChanged += (_, _) => ApplyResponsiveShell();
  Loaded += (_, _) => ApplyResponsiveShell();
 }

 private void ApplyResponsiveShell()
 {
  if (ActualWidth <= 0 || ContentColumns.ActualWidth <= 0) return;
  bool compact = ActualWidth < 1280;
  SidebarColumn.Width = TitleSidebarColumn.Width = new GridLength(194);
  double wantedListWidth = compact ? 260 : 294;
  // Preserve a manually dragged divider, while keeping enough room for editable detail cards.
  if (_preferredListWidth == null && EntityListColumn.Width.IsAbsolute && Math.Abs(EntityListColumn.Width.Value - _automaticListWidth) < 1)
   EntityListColumn.Width = new GridLength(wantedListWidth);
  _automaticListWidth = wantedListWidth;
  EntityListColumn.MaxWidth = Math.Max(EntityListColumn.MinWidth, Math.Min(420, ActualWidth - SidebarColumn.Width.Value - 5 - DetailColumn.MinWidth));
  if (_preferredListWidth is double preferredWidth)
   EntityListColumn.Width = new GridLength(Math.Min(preferredWidth, EntityListColumn.MaxWidth));
  if (EntityListColumn.ActualWidth > EntityListColumn.MaxWidth)
   EntityListColumn.Width = new GridLength(EntityListColumn.MaxWidth);
  DetailScroll.Padding = compact ? new Thickness(18, 18, 18, 24) : new Thickness(24, 20, 24, 24);
 }

 /// <summary>Native WPF render QA; DPI variants are off-screen render targets, not changes to the user's display.</summary>
 private async Task CaptureLayoutEvidenceAsync(string output)
 {
  if (App.Arg("--layout-matrix") != "true") return;
  string directory = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(output))!, "layout-matrix");
  Directory.CreateDirectory(directory);
  double originalWidth = Width, originalHeight = Height;
  GridLength originalListWidth = EntityListColumn.Width;
  double originalOffset = DetailScroll.VerticalOffset;
  var cases = new List<object>();
  var failures = new List<string>();
  var monitorDpi = VisualTreeHelper.GetDpi(this);
  CaptureFontEvidence(directory);
  AppDialog.RenderPreview(Path.Combine(directory, "dialog-preview.png"));
  try
  {
   foreach (var (width, height) in new[] { (1120, 740), (1240, 760), (1510, 990) })
   {
    Width = width; Height = height;
    EntityListColumn.Width = new GridLength(_automaticListWidth);
    await Dispatcher.InvokeAsync(() => { ApplyResponsiveShell(); UpdateLayout(); }, DispatcherPriority.Loaded);
    await Task.Delay(80);
    UpdateLayout();
    DetailScroll.ScrollToTop();
    UpdateLayout();
    Rect breadcrumb = WindowBounds(Breadcrumb), actions = WindowBounds(GameDateLabel);
    bool headerSeparated = breadcrumb.Right <= actions.Left + 1;
    bool detailFits = DetailHost.ActualWidth <= DetailScroll.ViewportWidth + 1;
    bool listFits = EntityGrid.Columns.Sum(column => column.ActualWidth) <= EntityGrid.ActualWidth + 1;
    var adaptive = Descendants<AdaptiveColumnsPanel>(DetailHost).Select(panel => new { columns = panel.ActualColumns, width = panel.ActualWidth, minColumnWidth = panel.MinColumnWidth }).ToArray();
    var croppedPortraits = Descendants<Image>(DetailHost).Count(image => image.Source != null && image.Stretch == Stretch.UniformToFill);
    var escapedCards = Descendants<AdaptiveColumnsPanel>(DetailHost).Count(panel => panel.Children.OfType<FrameworkElement>().Any(child => child.TransformToAncestor(panel).TransformBounds(new Rect(child.RenderSize)).Right > panel.ActualWidth + 1));
    if (!headerSeparated || !detailFits || !listFits || croppedPortraits != 0 || escapedCards != 0)
     failures.Add($"{width}×{height}: header={headerSeparated}, detail={detailFits}, list={listFits}, croppedPortraits={croppedPortraits}, escapedCards={escapedCards}");
    foreach (int percent in new[] { 100, 125, 150, 200 })
    {
     double scale = percent / 100d;
     string file = Path.Combine(directory, $"{width}x{height}-{percent}.png");
     CaptureAtDpi(file, scale);
     cases.Add(new { width, height, renderScale = scale, pixelWidth = (int)Math.Ceiling(ActualWidth * scale), pixelHeight = (int)Math.Ceiling(ActualHeight * scale), headerSeparated, detailFits, listFits, croppedPortraits, escapedCards, detailWidth = DetailHost.ActualWidth, scrollExtent = DetailScroll.ExtentHeight, scrollViewport = DetailScroll.ViewportHeight, adaptive, file });
    }
    DetailScroll.ScrollToBottom(); UpdateLayout();
    CaptureAtDpi(Path.Combine(directory, $"{width}x{height}-bottom.png"), 1);
   }
  }
  finally
  {
   Width = originalWidth; Height = originalHeight;
   EntityListColumn.Width = originalListWidth;
   ApplyResponsiveShell(); UpdateLayout();
   DetailScroll.ScrollToVerticalOffset(originalOffset);
  }
  File.WriteAllText(Path.Combine(directory, "report.json"), JsonSerializer.Serialize(new { success = failures.Count == 0, framework = "native WPF", gameWrites = false, displaySettingsChanged = false, dpiMethod = "Off-screen WPF RenderTargetBitmap at 96/120/144/192 DPI; window layout sizes are device-independent units. Actual monitor scale is reported separately.", monitorDpiX = monitorDpi.PixelsPerInchX, monitorDpiY = monitorDpi.PixelsPerInchY, cases, failures }, new JsonSerializerOptions { WriteIndented = true }));
  if (failures.Count != 0) throw new InvalidOperationException("Layout QA failed: " + string.Join("; ", failures));
  _qaChecks.Add("layout_matrix_3_sizes_4_render_scales");
 }

 private Rect WindowBounds(FrameworkElement element) => element.TransformToAncestor(this).TransformBounds(new Rect(element.RenderSize));

 private void CaptureFontEvidence(string directory)
 {
  const string sample = "선수 스태프 구단 도시 현재 능력 잠재 능력 변경 검토 우리 구단 크로스 패스 태클 판단력 예측력 민첩성 라스무스 호일룬 가나다힣 Højlund João Łukasz İbrahim 0123456789 — · / % £ €";
  var embedded = Typography.EmbeddedFont;
  var faces = new List<object>();
  bool embeddedFontsValid = true;
  foreach (FontWeight weight in new[] { FontWeights.Normal, FontWeights.Medium, FontWeights.SemiBold, FontWeights.Bold })
  {
   bool resolved = new Typeface(embedded, FontStyles.Normal, weight, FontStretches.Normal).TryGetGlyphTypeface(out var glyph);
   var missing = resolved ? sample.Where(character => !char.IsWhiteSpace(character) && !glyph!.CharacterToGlyphMap.ContainsKey(character)).Distinct().Select(character => $"{character} U+{(int)character:X4}").ToArray() : [];
   embeddedFontsValid &= resolved && missing.Length == 0 && glyph!.FontUri.ToString().StartsWith("pack://application", StringComparison.Ordinal) && glyph.Weight == weight;
   faces.Add(new { requestedFamily = embedded.Source, baseUri = embedded.BaseUri?.ToString(), weight = weight.ToString(), resolved, actualWeight = glyph?.Weight.ToString(), fontUri = glyph?.FontUri.ToString(), familyNames = glyph?.FamilyNames.Values, faceNames = glyph?.FaceNames.Values, missingSampleGlyphs = missing });
  }
  string[] textInView = Descendants<TextBlock>(this).Select(block => block.Text).Where(value => !string.IsNullOrEmpty(value)).ToArray();
  int replacementCharacters = textInView.Sum(value => value.Count(character => character == '\uFFFD'));
  var visibleTextWeights = Descendants<TextBlock>(this).Where(block => block.IsVisible && !string.IsNullOrWhiteSpace(block.Text)).GroupBy(block => block.FontWeight.ToString()).Select(group => new { weight = group.Key, count = group.Count(), samples = group.Select(block => block.Text).Take(5).ToArray() }).ToArray();
  var inputWeights = Descendants<TextBox>(this).Where(input => input.IsVisible).GroupBy(input => input.FontWeight.ToString()).Select(group => new { weight = group.Key, count = group.Count() }).ToArray();
  File.WriteAllText(Path.Combine(directory, "font-report.json"), JsonSerializer.Serialize(new { success = embeddedFontsValid && replacementCharacters == 0, requestedFamily = FontFamily.Source, baseUri = FontFamily.BaseUri?.ToString(), defaultWeight = FontWeight.ToString(), configuredRendering = TextOptions.GetTextRenderingMode(this).ToString(), formatting = TextOptions.GetTextFormattingMode(this).ToString(), UseLayoutRounding, SnapsToDevicePixels, replacementCharacters, sample, faces, visibleTextWeights, inputWeights, note = "Embedded glyph resolution and off-screen native WPF raster samples verify packaging/coverage. Monitor subpixel order, GPU driver and physical display appearance require confirmation on the affected display." }, new JsonSerializerOptions { WriteIndented = true }));
  if (!embeddedFontsValid || replacementCharacters != 0) throw new InvalidOperationException("Embedded Pretendard glyph validation failed; inspect font-report.json.");
  _qaChecks.Add("pretendard_regular_medium_semibold_bold_embedded_glyphs");
  foreach (int percent in new[] { 100, 125, 150, 200 })
  {
   double scale = percent / 100d;
   var visual = new DrawingVisual();
   TextOptions.SetTextFormattingMode(visual, TextFormattingMode.Display);
   using (DrawingContext drawing = visual.RenderOpen())
   {
    drawing.DrawRectangle(Brush("#10141D"), null, new Rect(0, 0, 1000, 320));
    double y = 20;
    foreach (TextRenderingMode mode in new[] { TextRenderingMode.Grayscale, TextRenderingMode.ClearType })
    {
     // Use separate child visuals so each rendering mode applies to its own native glyph runs.
     var textVisual = new DrawingVisual();
     TextOptions.SetTextFormattingMode(textVisual, TextFormattingMode.Display);
     TextOptions.SetTextRenderingMode(textVisual, mode);
     using (DrawingContext textDrawing = textVisual.RenderOpen())
     {
      foreach (var (value, weight, size) in new[] { ($"Pretendard Bold 700 · {mode} · {percent}%", FontWeights.Bold, 17d), ("Medium 500 · 선수와 구단, 모든 가능성을 한곳에서  ·  현재 능력 / 잠재 능력", FontWeights.Medium, 13d), ("SemiBold 600 · 라스무스 호일룬 · 기술 · 정신 · 신체 · 판단력 · 183 / 195", FontWeights.SemiBold, 13d), ("Medium 500 · Rasmus Højlund · João · Łukasz · İbrahim · GBP £12,345,678", FontWeights.Medium, 12d) })
      {
       var text = new FormattedText(value, System.Globalization.CultureInfo.GetCultureInfo("ko-KR"), FlowDirection.LeftToRight, new Typeface(embedded, FontStyles.Normal, weight, FontStretches.Normal), size, Brush("#EBF0F8"), scale);
       textDrawing.DrawText(text, new Point(20, y));
       y += 29;
      }
     }
     visual.Children.Add(textVisual);
     y += 20;
    }
   }
   var bitmap = new RenderTargetBitmap((int)(1000 * scale), (int)(320 * scale), 96 * scale, 96 * scale, PixelFormats.Pbgra32);
   bitmap.Render(visual);
   var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
   using var stream = File.Create(Path.Combine(directory, $"font-samples-{percent}.png")); encoder.Save(stream);
  }
 }

 private void CaptureAtDpi(string path, double scale)
 {
  var bitmap = new RenderTargetBitmap((int)Math.Ceiling(ActualWidth * scale), (int)Math.Ceiling(ActualHeight * scale), 96 * scale, 96 * scale, PixelFormats.Pbgra32);
  bitmap.Render(this);
  var encoder = new PngBitmapEncoder();
  encoder.Frames.Add(BitmapFrame.Create(bitmap));
  using var stream = File.Create(path);
  encoder.Save(stream);
 }

 private static IEnumerable<T> Descendants<T>(DependencyObject parent) where T : DependencyObject
 {
  for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++)
  {
   DependencyObject child = VisualTreeHelper.GetChild(parent, i);
   if (child is T found) yield return found;
   foreach (T descendant in Descendants<T>(child)) yield return descendant;
  }
 }
}
