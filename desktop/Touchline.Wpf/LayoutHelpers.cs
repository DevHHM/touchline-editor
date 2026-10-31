using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Touchline.Wpf;

public static class Typography
{
 // WPF needs a base URI for a packaged font directory. An absolute URI in the
 // family-name string silently falls back instead of resolving these font faces.
 public static FontFamily EmbeddedFont { get; } = new(new Uri("pack://application:,,,/"), "./Fonts/#Pretendard");
 public static FontFamily AppFont { get; } = new(new Uri("pack://application:,,,/"), "./Fonts/#Pretendard, Malgun Gothic, Segoe UI");
 private static bool _registered;

 public static void Initialize(Window window)
 {
  if (!_registered)
  {
   _registered = true;
   EventManager.RegisterClassHandler(typeof(Window), FrameworkElement.LoadedEvent, new RoutedEventHandler((sender, args) =>
   {
    if (sender is Window loaded && ReferenceEquals(args.OriginalSource, loaded)) Configure(loaded);
   }));
  }
  Configure(window);
 }

 private static void Configure(Window window)
 {
  window.FontFamily = AppFont;
  window.SetResourceReference(Control.FontWeightProperty, "AppBodyFontWeight");
  window.SetResourceReference(TextOptions.TextRenderingModeProperty, "AppTextRenderingMode");
  TextOptions.SetTextFormattingMode(window, TextFormattingMode.Display);
  window.UseLayoutRounding = true;
  window.SnapsToDevicePixels = true;
 }
}

/// <summary>Keeps editable cards readable as the detail pane or monitor work area changes.</summary>
public sealed class AdaptiveColumnsPanel : Panel
{
 public static readonly DependencyProperty MinColumnWidthProperty = DependencyProperty.Register(nameof(MinColumnWidth), typeof(double), typeof(AdaptiveColumnsPanel), new FrameworkPropertyMetadata(210d, FrameworkPropertyMetadataOptions.AffectsMeasure));
 public static readonly DependencyProperty MaxColumnsProperty = DependencyProperty.Register(nameof(MaxColumns), typeof(int), typeof(AdaptiveColumnsPanel), new FrameworkPropertyMetadata(3, FrameworkPropertyMetadataOptions.AffectsMeasure));
 public static readonly DependencyProperty ColumnGapProperty = DependencyProperty.Register(nameof(ColumnGap), typeof(double), typeof(AdaptiveColumnsPanel), new FrameworkPropertyMetadata(14d, FrameworkPropertyMetadataOptions.AffectsMeasure));
 public double MinColumnWidth { get => (double)GetValue(MinColumnWidthProperty); set => SetValue(MinColumnWidthProperty, value); }
 public int MaxColumns { get => (int)GetValue(MaxColumnsProperty); set => SetValue(MaxColumnsProperty, value); }
 public double ColumnGap { get => (double)GetValue(ColumnGapProperty); set => SetValue(ColumnGapProperty, value); }
 public int ActualColumns { get; private set; } = 1;

 private int ColumnCount(double width) => Math.Max(1, Math.Min(Math.Min(Math.Max(1, MaxColumns), Math.Max(1, InternalChildren.Count)), (int)Math.Floor((width + Math.Max(0, ColumnGap)) / (Math.Max(1, MinColumnWidth) + Math.Max(0, ColumnGap)))));
 private double AvailableWidth(double width) => double.IsInfinity(width) ? Math.Max(1, MinColumnWidth) * Math.Max(1, Math.Min(MaxColumns, InternalChildren.Count)) + Math.Max(0, ColumnGap) * Math.Max(0, Math.Min(MaxColumns, InternalChildren.Count) - 1) : Math.Max(0, width);

 protected override Size MeasureOverride(Size availableSize)
 {
  double width = AvailableWidth(availableSize.Width);
  int columns = ColumnCount(width);
  double cellWidth = Math.Max(0, (width - Math.Max(0, ColumnGap) * (columns - 1)) / columns);
  var heights = new double[columns];
  for (int i = 0; i < InternalChildren.Count; i++)
  {
   var child = InternalChildren[i];
   child.Measure(new Size(cellWidth, double.PositiveInfinity));
   heights[i % columns] += child.DesiredSize.Height;
  }
  return new Size(width, heights.Max());
 }

 protected override Size ArrangeOverride(Size finalSize)
 {
  ActualColumns = ColumnCount(finalSize.Width);
  double gap = Math.Max(0, ColumnGap);
  double cellWidth = Math.Max(0, (finalSize.Width - gap * (ActualColumns - 1)) / ActualColumns);
  var heights = new double[ActualColumns];
  for (int i = 0; i < InternalChildren.Count; i++)
  {
   var child = InternalChildren[i];
   int column = i % ActualColumns;
   child.Arrange(new Rect(column * (cellWidth + gap), heights[column], cellWidth, child.DesiredSize.Height));
   heights[column] += child.DesiredSize.Height;
  }
  return finalSize;
 }
}
