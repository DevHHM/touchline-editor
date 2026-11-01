namespace Touchline.Wpf;

/// <summary>A canceled workbench close must not begin replacing the current session.</summary>
public static class SnapshotTransition
{
 public static async Task<bool> RunAsync(Func<Task> closeWorkbenches, Action beginTransition, Func<Task> loadSnapshot)
 {
  try { await closeWorkbenches(); }
  catch(OperationCanceledException) { return false; }
  beginTransition();
  await loadSnapshot();
  return true;
 }
}
