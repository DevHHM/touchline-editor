using System.IO;
using System.Text.Json;

namespace Touchline.Wpf;

public readonly record struct DesktopRect(double Left, double Top, double Width, double Height)
{
 public double Right => Left + Width;
 public double Bottom => Top + Height;
 public bool IsValid => double.IsFinite(Left) && double.IsFinite(Top) && double.IsFinite(Width) && double.IsFinite(Height) && Width > 0 && Height > 0 && Math.Abs(Left) < 1_000_000 && Math.Abs(Top) < 1_000_000 && Width <= 100_000 && Height <= 100_000;
 public double IntersectionArea(DesktopRect other) => Math.Max(0, Math.Min(Right, other.Right) - Math.Max(Left, other.Left)) * Math.Max(0, Math.Min(Bottom, other.Bottom) - Math.Max(Top, other.Top));
 public double DistanceSquared(DesktopRect other)
 {
  double x = Math.Max(0, Math.Max(other.Left - Right, Left - other.Right));
  double y = Math.Max(0, Math.Max(other.Top - Bottom, Top - other.Bottom));
  return x * x + y * y;
 }
}

public sealed record DesktopMonitor(string Device, DesktopRect Bounds, DesktopRect WorkArea, double Scale);
public sealed record DesktopWindowPreferences
{
 public int Version { get; init; } = 1;
 public DesktopRect NormalBounds { get; init; }
 public string MonitorDevice { get; init; } = "";
 public double DpiScale { get; init; } = 1;
 public bool Maximized { get; init; }
 public double? ListWidth { get; init; }
 public bool IsValid => Version == 1 && NormalBounds.IsValid && double.IsFinite(DpiScale) && DpiScale is >= .5 and <= 8;
 public double? ValidListWidth => ListWidth is double width && double.IsFinite(width) ? Math.Clamp(width, 240, 420) : null;

 public DesktopMonitor SelectMonitor(IReadOnlyList<DesktopMonitor> monitors)
 {
  if (monitors.Count == 0) throw new ArgumentException("No connected monitors.", nameof(monitors));
  return monitors.FirstOrDefault(monitor => monitor.Device == MonitorDevice)
   ?? monitors.OrderByDescending(monitor => NormalBounds.IntersectionArea(monitor.WorkArea))
    .ThenBy(monitor => NormalBounds.DistanceSquared(monitor.WorkArea)).First();
 }

 public DesktopRect FitTo(DesktopMonitor monitor)
 {
  if (!IsValid || !monitor.WorkArea.IsValid || !double.IsFinite(monitor.Scale) || monitor.Scale <= 0) throw new ArgumentException("Invalid window or monitor bounds.");
  var area = monitor.WorkArea;
  double scale = monitor.Scale / DpiScale;
  double width = Math.Min(area.Width, Math.Max(Math.Min(1120 * monitor.Scale, area.Width), NormalBounds.Width * scale));
  double height = Math.Min(area.Height, Math.Max(Math.Min(740 * monitor.Scale, area.Height), NormalBounds.Height * scale));
  return new(Math.Clamp(NormalBounds.Left, area.Left, area.Right - width), Math.Clamp(NormalBounds.Top, area.Top, area.Bottom - height), width, height);
 }

 public static DesktopWindowPreferences? Load(string path)
 {
  try
  {
   if (!File.Exists(path)) return null;
   var saved = JsonSerializer.Deserialize<DesktopWindowPreferences>(File.ReadAllText(path));
   return saved?.IsValid == true ? saved : null;
  }
  catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException) { return null; }
 }

 public void Save(string path)
 {
  if (!IsValid) return;
  string fullPath = Path.GetFullPath(path);
  Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
  string temporary = fullPath + "." + Guid.NewGuid().ToString("N") + ".tmp";
  try
  {
   File.WriteAllText(temporary, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
   File.Move(temporary, fullPath, true);
  }
  finally { if (File.Exists(temporary)) File.Delete(temporary); }
 }
}
