namespace Touchline.Wpf;

public sealed record TutorialStep(string TargetKey, string Label, string Title, string Body, string Hint);

/// <summary>The guide describes real controls; advancing a step never invokes them.</summary>
public static class TutorialTour
{
 public static IReadOnlyList<TutorialStep> Steps { get; } = Array.AsReadOnly(new TutorialStep[]
 {
  new("connection", "연결", "여기서 FM과 연결해요",
   "FM에서 세이브를 불러온 다음, 왼쪽 아래 ‘FM 연결’을 누르세요. 연결되면 현재 선수와 구단 정보를 확인할 수 있어요.",
   "지금은 위치만 안내합니다. 안내를 넘겨도 FM 연결이나 게임 수정은 실행되지 않아요."),
  new("navigation", "대상 선택", "원하는 대상을 골라보세요",
   "왼쪽 메뉴에서 선수·스태프·구단을 고르세요. 기타 인물과 도시도 같은 방식으로 찾아볼 수 있어요.",
   "대상을 고른 뒤 목록의 이름을 열면 상세 정보와 수정 가능한 항목이 나옵니다."),
  new("search", "검색", "검색·필터는 이 버튼이에요",
   "이름뿐 아니라 국적·나이·포지션·능력치 등 여러 조건을 조합해 원하는 대상을 찾을 수 있어요.",
   "연결 전에는 검색 버튼이 비활성화됩니다. 연결 후 Ctrl+Shift+F로도 열 수 있어요."),
  new("review", "변경 검토", "수정한 내용은 먼저 검토해요",
   "값을 바꾼 다음 오른쪽 위 ‘변경 검토’를 누르세요. 바뀌기 전과 후를 확인하고, 검토 창에서 적용을 선택하면 게임에 반영됩니다.",
   "입력하거나 미리 보는 것만으로 게임은 바뀌지 않아요. 수정할 내용이 있을 때 버튼이 활성화됩니다."),
  new("history", "변경 이력", "어떤 값을 바꿨는지 확인해요",
   "‘변경 이력’에서 적용한 수정 기록을 볼 수 있어요. 되돌리기를 지원하는 작업은 현재 상태를 확인한 뒤 복구할 수 있습니다.",
   "세이브를 다시 불러온 뒤에는 새 연결의 정보를 기준으로 확인하세요."),
  new("help", "도움말", "필요할 때 안내를 다시 열어요",
   "오른쪽 위 ‘?’에서 도움말과 진단을 열 수 있어요. ‘처음 시작하기’를 누르면 이 안내를 다시 볼 수 있습니다.",
   "오류 내용은 사용자가 해당 오류의 공유에 동의한 경우에만 GitHub로 전송됩니다.")
 });
}

public readonly record struct TutorialRect(double X, double Y, double Width, double Height)
{
 public double Right => X + Width;
 public double Bottom => Y + Height;
}

/// <summary>Viewport-relative placement, independent of WPF and display scaling.</summary>
public static class TutorialTourPlacement
{
 private const double Padding = 12, Gap = 20, SafeExtent = 10_000_000;

 public static TutorialRect Place(TutorialRect viewport, TutorialRect target, double cardWidth, double cardHeight)
 {
  viewport = Normalize(viewport);
  double px = Math.Min(Padding, viewport.Width / 2), py = Math.Min(Padding, viewport.Height / 2);
  var interior = new TutorialRect(viewport.X + px, viewport.Y + py,
   Math.Max(0, viewport.Width - 2 * px), Math.Max(0, viewport.Height - 2 * py));
  double width = Math.Min(Positive(cardWidth, 360), interior.Width);
  double height = Math.Min(Positive(cardHeight, 300), interior.Height);
  target = VisiblePart(Normalize(target), viewport);
  double cx = target.X + target.Width / 2, cy = target.Y + target.Height / 2;
  TutorialRect[] candidates =
  [
   new(target.Right + Gap, cy - height / 2, width, height),
   new(target.X - Gap - width, cy - height / 2, width, height),
   new(cx - width / 2, target.Bottom + Gap, width, height),
   new(cx - width / 2, target.Y - Gap - height, width, height)
  ];
  // Preserve the preferred side while allowing the perpendicular axis to fit.
  if (candidates[0].Right <= interior.Right) return Clamp(candidates[0], interior);
  if (candidates[1].X >= interior.X) return Clamp(candidates[1], interior);
  if (candidates[2].Bottom <= interior.Bottom) return Clamp(candidates[2], interior);
  if (candidates[3].Y >= interior.Y) return Clamp(candidates[3], interior);

  // A small viewport can make separation impossible. Keep all controls on screen
  // and use the placement which obscures the least of the target.
  return candidates.Select(candidate => Clamp(candidate, interior))
   .OrderBy(candidate => Overlap(candidate, target)).First();
 }

 private static TutorialRect Normalize(TutorialRect rectangle) => new(
  Coordinate(rectangle.X), Coordinate(rectangle.Y),
  Math.Min(Positive(rectangle.Width, 0), SafeExtent), Math.Min(Positive(rectangle.Height, 0), SafeExtent));

 private static double Coordinate(double value) => double.IsFinite(value) ? Math.Clamp(value, -SafeExtent, SafeExtent) : 0;
 private static double Positive(double value, double fallback) => double.IsFinite(value) && value > 0 ? value : fallback;

 private static TutorialRect VisiblePart(TutorialRect rectangle, TutorialRect viewport)
 {
  double x = Math.Clamp(rectangle.X, viewport.X, viewport.Right), y = Math.Clamp(rectangle.Y, viewport.Y, viewport.Bottom);
  double right = Math.Clamp(rectangle.Right, x, viewport.Right), bottom = Math.Clamp(rectangle.Bottom, y, viewport.Bottom);
  return new(x, y, right - x, bottom - y);
 }

 private static TutorialRect Clamp(TutorialRect rectangle, TutorialRect viewport) => rectangle with
 {
  X = Math.Clamp(rectangle.X, viewport.X, Math.Max(viewport.X, viewport.Right - rectangle.Width)),
  Y = Math.Clamp(rectangle.Y, viewport.Y, Math.Max(viewport.Y, viewport.Bottom - rectangle.Height))
 };

 private static double Overlap(TutorialRect a, TutorialRect b) =>
  Math.Max(0, Math.Min(a.Right, b.Right) - Math.Max(a.X, b.X)) *
  Math.Max(0, Math.Min(a.Bottom, b.Bottom) - Math.Max(a.Y, b.Y));
}
