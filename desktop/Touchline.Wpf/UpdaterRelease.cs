using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Touchline.Wpf;

public readonly record struct UpdateVersion(int Major, int Minor, int Patch) : IComparable<UpdateVersion>
{
 public static UpdateVersion Parse(string value)
 {
  var match = Regex.Match(value, @"^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$", RegexOptions.CultureInvariant);
  if (!match.Success) throw new InvalidDataException("지원하지 않는 릴리스 버전입니다.");
  try { return new(int.Parse(match.Groups[1].Value, CultureInfo.InvariantCulture), int.Parse(match.Groups[2].Value, CultureInfo.InvariantCulture), int.Parse(match.Groups[3].Value, CultureInfo.InvariantCulture)); }
  catch (OverflowException ex) { throw new InvalidDataException("릴리스 버전이 너무 큽니다.", ex); }
 }
 public int CompareTo(UpdateVersion other) => Major != other.Major ? Major.CompareTo(other.Major) : Minor != other.Minor ? Minor.CompareTo(other.Minor) : Patch.CompareTo(other.Patch);
 public override string ToString() => $"{Major}.{Minor}.{Patch}";
}

public sealed record UpdateRelease(UpdateVersion Version, string Tag, Uri Page, Uri Archive, Uri Checksum, long ArchiveBytes, string? GitHubDigest);

public static class UpdateFeed
{
 public const string Repository = "DevHHM/touchline-editor";
 public static readonly Uri Feed = new($"https://api.github.com/repos/{Repository}/releases?per_page=30");
 public static string ArchiveName(UpdateVersion version) => $"Touchline-Editor-{version}-win-x64.zip";
 public static bool IsAssetUri(Uri uri, string tag, string name) => uri.Scheme == "https" && uri.IsDefaultPort && string.IsNullOrEmpty(uri.UserInfo) && uri.Host.Equals("github.com", StringComparison.OrdinalIgnoreCase)
  && uri.AbsolutePath == $"/{Repository}/releases/download/{tag}/{name}" && uri.Query == "" && uri.Fragment == "";
 public static bool IsCdnUri(Uri uri) => uri.Scheme == "https" && uri.IsDefaultPort && string.IsNullOrEmpty(uri.UserInfo) && uri.Fragment == "" && uri.Host is "release-assets.githubusercontent.com" or "objects.githubusercontent.com";
 public static UpdateRelease? Select(string json, UpdateVersion current, bool includePreview)
 {
  using var document = JsonDocument.Parse(json);
  if (document.RootElement.ValueKind != JsonValueKind.Array) throw new InvalidDataException("릴리스 응답 형식이 올바르지 않습니다.");
  var releases = new List<UpdateRelease>();
  foreach (var release in document.RootElement.EnumerateArray())
  {
   if (release.GetProperty("draft").GetBoolean() || (!includePreview && release.GetProperty("prerelease").GetBoolean())) continue;
   var tag = release.GetProperty("tag_name").GetString() ?? "";
   UpdateVersion version;
   try { version = UpdateVersion.Parse(tag); } catch (InvalidDataException) { continue; }
   if (version.CompareTo(current) <= 0) continue;
   string name = ArchiveName(version);
   var assets = release.GetProperty("assets").EnumerateArray().ToArray();
   var zip = assets.Where(a => a.GetProperty("name").GetString() == name).ToArray();
   var hash = assets.Where(a => a.GetProperty("name").GetString() == name + ".sha256").ToArray();
   if (zip.Length != 1 || hash.Length != 1) continue; // An incompletely published release is not offered.
   var archive = new Uri(zip[0].GetProperty("browser_download_url").GetString()!);
   var checksum = new Uri(hash[0].GetProperty("browser_download_url").GetString()!);
   if (!IsAssetUri(archive, tag, name) || !IsAssetUri(checksum, tag, name + ".sha256")) throw new InvalidDataException("공식 저장소 밖의 업데이트 주소를 거부했습니다.");
   long size = zip[0].GetProperty("size").GetInt64();
   if (size <= 0 || size > UpdateDownload.MaximumArchiveBytes) throw new InvalidDataException("업데이트 파일 크기가 허용 범위를 벗어났습니다.");
   string? digest = zip[0].TryGetProperty("digest", out var d) && d.ValueKind == JsonValueKind.String ? d.GetString() : null;
   if (digest != null && !Regex.IsMatch(digest, "^sha256:[0-9a-fA-F]{64}$")) throw new InvalidDataException("릴리스 해시 형식이 올바르지 않습니다.");
   releases.Add(new(version, tag, new Uri($"https://github.com/{Repository}/releases/tag/{tag}"), archive, checksum, size, digest));
  }
  return releases.OrderByDescending(r => r.Version).FirstOrDefault();
 }
 public static string ParseChecksum(string content, string expectedName)
 {
  var match = Regex.Match(content.Trim().TrimStart('\uFEFF'), @"^([0-9a-fA-F]{64})\s+\*?([^\r\n]+)$", RegexOptions.CultureInvariant);
  if (!match.Success || match.Groups[2].Value != expectedName) throw new InvalidDataException("SHA256 파일이 릴리스 ZIP과 일치하지 않습니다.");
  return match.Groups[1].Value.ToLowerInvariant();
 }
}
