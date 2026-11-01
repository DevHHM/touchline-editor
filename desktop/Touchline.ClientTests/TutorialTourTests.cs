using Touchline.Wpf;

internal static class TutorialTourTests
{
 public static void Run(Action<bool, string> check)
 {
  var steps = TutorialTour.Steps;
  check(steps.Count == 6 && steps.Select(step => step.TargetKey).Distinct(StringComparer.Ordinal).Count() == steps.Count,
   "guided_tutorial_has_six_unique_real_targets");
  check(steps.Select(step => step.TargetKey).SequenceEqual(new[] { "connection", "navigation", "search", "review", "history", "help" }),
   "guided_tutorial_follows_connect_find_review_history_help_workflow");
  check(steps.All(step => !string.IsNullOrWhiteSpace(step.Title) && !string.IsNullOrWhiteSpace(step.Body) && !string.IsNullOrWhiteSpace(step.Hint)),
   "guided_tutorial_every_step_has_instruction_and_context");
  check(steps[0].Hint.Contains("실행되지") && steps[3].Hint.Contains("게임은 바뀌지") && steps[5].Hint.Contains("동의한 경우에만"),
   "guided_tutorial_explains_no_actions_and_explicit_error_consent");

  var viewport = new TutorialRect(0, 0, 1200, 800);
  var left = new TutorialRect(16, 700, 160, 36);
  var right = new TutorialRect(1050, 80, 130, 36);
  var below = new TutorialRect(350, 180, 500, 80);
  var above = new TutorialRect(350, 600, 500, 80);
  var rightCard = TutorialTourPlacement.Place(viewport, left, 360, 300);
  var leftCard = TutorialTourPlacement.Place(viewport, right, 360, 300);
  var belowCard = TutorialTourPlacement.Place(viewport, below, 360, 300);
  var aboveCard = TutorialTourPlacement.Place(viewport, above, 360, 300);
  check(rightCard.X >= left.Right + 20 && !Overlaps(rightCard, left), "guided_tutorial_left_control_places_card_to_right");
  check(leftCard.Right <= right.X - 20 && !Overlaps(leftCard, right), "guided_tutorial_right_control_places_card_to_left");
  check(belowCard.Y >= below.Bottom + 20 && !Overlaps(belowCard, below), "guided_tutorial_wide_control_places_card_below");
  check(aboveCard.Bottom <= above.Y - 20 && !Overlaps(aboveCard, above), "guided_tutorial_bottom_control_places_card_above");

  TutorialRect[] targets =
  [
   new(0, 0, 30, 30), new(1170, 0, 30, 30), new(0, 770, 30, 30), new(1170, 770, 30, 30),
   new(400, 300, 100, 40), new(-300, -100, 200, 40), new(1600, 1000, 30, 30), new(0, 0, 1200, 800)
  ];
  check(targets.All(target => Inside(TutorialTourPlacement.Place(viewport, target, 360, 300), viewport, 12)),
   "guided_tutorial_cards_stay_inside_viewport_at_all_edges_and_offscreen_targets");

  TutorialRect[] sizes = [new(0, 0, 800, 540), new(0, 0, 1280, 720), new(0, 0, 1920, 1080), new(0, 0, 540, 800), new(50, 25, 800, 540)];
  check(sizes.All(size => Inside(TutorialTourPlacement.Place(size, new(size.Right - 80, size.Y + 30, 60, 30), 390, 320), size, 12)),
   "guided_tutorial_resizing_and_nonzero_origins_keep_card_visible");
  var smallViewport = new TutorialRect(0, 0, 240, 180);
  var small = TutorialTourPlacement.Place(smallViewport, new(10, 10, 50, 30), 400, 360);
  check(small.Width == 216 && small.Height == 156 && Inside(small, smallViewport, 12),
   "guided_tutorial_small_viewport_clamps_card_dimensions_instead_of_clipping_controls");
  var tiny = TutorialTourPlacement.Place(new(0, 0, 10, 8), new(0, 0, 20, 20), 400, 300);
  check(tiny.Width == 0 && tiny.Height == 0 && tiny.X == 5 && tiny.Y == 4,
   "guided_tutorial_tiny_viewport_never_produces_negative_dimensions");
  var resized = TutorialTourPlacement.Place(new(0, 0, 800, 540), right, 360, 300);
  check(resized != leftCard && Inside(resized, new(0, 0, 800, 540), 12),
   "guided_tutorial_recomputes_anchor_after_window_resize");

  TutorialRect[] malformed =
  [
   new(double.NaN, double.PositiveInfinity, -1, double.NegativeInfinity),
   new(double.MaxValue, -double.MaxValue, double.MaxValue, double.MaxValue),
   new(0, 0, 0, 0)
  ];
  check(malformed.All(target => Finite(TutorialTourPlacement.Place(viewport, target, double.NaN, double.PositiveInfinity))),
   "guided_tutorial_malformed_target_or_card_size_never_produces_nan");
  check(malformed.All(size => Finite(TutorialTourPlacement.Place(size, left, 360, 300))),
   "guided_tutorial_malformed_viewport_is_finite_and_nonnegative");
  check(Inside(TutorialTourPlacement.Place(viewport, new(-100, 100, 150, 40), 360, 300), viewport, 12),
   "guided_tutorial_partially_clipped_target_keeps_annotation_inside_window");
 }

 private static bool Finite(TutorialRect r) => double.IsFinite(r.X) && double.IsFinite(r.Y) && double.IsFinite(r.Width) && double.IsFinite(r.Height) && r.Width >= 0 && r.Height >= 0;
 private static bool Inside(TutorialRect card, TutorialRect viewport, double padding) => card.X >= viewport.X + padding && card.Y >= viewport.Y + padding && card.Right <= viewport.Right - padding && card.Bottom <= viewport.Bottom - padding;
 private static bool Overlaps(TutorialRect a, TutorialRect b) => a.X < b.Right && a.Right > b.X && a.Y < b.Bottom && a.Bottom > b.Y;
}
