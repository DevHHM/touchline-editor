using System.Collections.Concurrent;
using System.Text.RegularExpressions;

namespace Touchline.Wpf;

/// <summary>Exact position tokens in goalkeeper-to-striker order; staff roles remain lexical.</summary>
public static class PositionOrder
{
 public const int UnknownRank = int.MaxValue;
 private static readonly Dictionary<string, int> Ranks = new(StringComparer.OrdinalIgnoreCase)
 {
  ["GK"] = 0, ["SW"] = 1, ["DR"] = 2, ["DC"] = 3, ["DL"] = 4,
  ["WBR"] = 5, ["WBL"] = 6, ["DM"] = 7, ["MR"] = 8, ["MC"] = 9,
  ["ML"] = 10, ["AMR"] = 11, ["AMC"] = 12, ["AML"] = 13, ["ST"] = 14
 };
 private static readonly Regex Separators = new(@"[·/,\s]+", RegexOptions.CultureInvariant);
 private static readonly ConcurrentDictionary<string, int> RankCache = new(StringComparer.OrdinalIgnoreCase);

 public static int Rank(string? role)
 {
  if (string.IsNullOrWhiteSpace(role)) return UnknownRank;
  return RankCache.GetOrAdd(role, ComputeRank);
 }

 private static int ComputeRank(string role)
 {
  int first = UnknownRank;
  foreach (string token in Separators.Split(role.Trim())) if (Ranks.TryGetValue(token, out int rank)) first = Math.Min(first, rank);
  return first;
 }

 public static int Compare(EntityRecord a, EntityRecord b, bool descending = false)
 {
  bool aPlayer = a.Kind == "Player", bPlayer = b.Kind == "Player";
  // Keep mixed workbench lists transitive; this bucket order is independent of direction.
  if (aPlayer != bPlayer) return aPlayer ? -1 : 1;
  int order;
  if (aPlayer)
  {
   int aRank = a.PositionRank, bRank = b.PositionRank;
   if (aRank == UnknownRank || bRank == UnknownRank)
   {
    if (aRank != bRank) return aRank == UnknownRank ? 1 : -1;
    order = 0;
   }
   else order = aRank.CompareTo(bRank);
  }
  else order = StringComparer.CurrentCultureIgnoreCase.Compare(a.Role, b.Role);
  if (order != 0) return descending ? -order : order;
  int identity = a.Id.CompareTo(b.Id);
  return identity != 0 ? identity : StringComparer.Ordinal.Compare(a.Kind, b.Kind);
 }
}
