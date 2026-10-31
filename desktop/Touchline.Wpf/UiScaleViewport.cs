using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Touchline.Wpf;

/// <summary>Scales a bounded layout so nested grids keep their own scrolling/virtualization.
/// On a small display the outer scrollbars expose the complete minimum-sized workspace.</summary>
public sealed class UiScaleViewport : ScrollViewer
{
 public static readonly DependencyProperty ScaleProperty = DependencyProperty.Register(nameof(Scale), typeof(double), typeof(UiScaleViewport), new FrameworkPropertyMetadata(1d, FrameworkPropertyMetadataOptions.AffectsMeasure));
 public double Scale { get => (double)GetValue(ScaleProperty); set => SetValue(ScaleProperty, value); }
 public double MinimumContentWidth { get; set; } = 320;
 public double MinimumContentHeight { get; set; } = 200;
 private readonly ScaleTransform _transform = new();

 public UiScaleViewport()
 {
  HorizontalScrollBarVisibility = ScrollBarVisibility.Auto;
  VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
  CanContentScroll = false;
  SetResourceReference(ScaleProperty, "AppUiScale");
 }

 protected override Size MeasureOverride(Size constraint)
 {
  if (Content is FrameworkElement surface)
  {
   double scale = double.IsFinite(Scale) && Scale > 0 ? Scale : 1;
   _transform.ScaleX = _transform.ScaleY = scale;
   surface.LayoutTransform = _transform;
   double width = constraint.Width, height = constraint.Height;
   // One scrollbar can require the other. Resolve both before constraining the child.
   bool horizontal = MinimumContentWidth * scale > width;
   bool vertical = MinimumContentHeight * scale > height;
   for (int i = 0; i < 2; i++)
   {
    horizontal |= MinimumContentWidth * scale > width - (vertical ? SystemParameters.VerticalScrollBarWidth : 0);
    vertical |= MinimumContentHeight * scale > height - (horizontal ? SystemParameters.HorizontalScrollBarHeight : 0);
   }
   surface.Width = double.IsFinite(width) ? Math.Max(MinimumContentWidth, (width - (vertical ? SystemParameters.VerticalScrollBarWidth : 0)) / scale) : double.NaN;
   surface.Height = double.IsFinite(height) ? Math.Max(MinimumContentHeight, (height - (horizontal ? SystemParameters.HorizontalScrollBarHeight : 0)) / scale) : double.NaN;
  }
  return base.MeasureOverride(constraint);
 }
}
