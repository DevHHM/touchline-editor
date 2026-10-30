using System.Net;
using System.IO;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;

namespace Touchline.Wpf;

/// <summary>Public GitHub requests only. No credentials, cookies or ambient redirect policy.</summary>
public sealed class UpdateDownload : IDisposable
{
 public const long MaximumArchiveBytes = 1024L * 1024 * 1024;
 private readonly HttpClient _client;
 public UpdateDownload(HttpMessageHandler? handler = null)
 {
  _client = new(handler ?? new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseDefaultCredentials = false }) { Timeout = TimeSpan.FromMinutes(20) };
  _client.DefaultRequestHeaders.UserAgent.ParseAdd("Touchline-Editor-Updater/1.0");
  _client.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
  _client.DefaultRequestHeaders.Add("X-GitHub-Api-Version", "2026-03-10");
 }
 public async Task<UpdateRelease?> CheckAsync(UpdateVersion current, bool preview, CancellationToken cancellation)
 {
  using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellation); timeout.CancelAfter(TimeSpan.FromSeconds(20));
  using var response = await GetAsync(UpdateFeed.Feed, false, timeout.Token);
  var data = await ReadBoundedAsync(response, 2 * 1024 * 1024, timeout.Token);
  return UpdateFeed.Select(Encoding.UTF8.GetString(data), current, preview);
 }
 private async Task<HttpResponseMessage> GetAsync(Uri initial, bool asset, CancellationToken cancellation)
 {
  Uri current = initial;
  for (int redirect = 0; redirect <= 5; redirect++)
  {
   using var request = new HttpRequestMessage(HttpMethod.Get, current);
   var response = await _client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellation);
   if (response.StatusCode is HttpStatusCode.Moved or HttpStatusCode.Redirect or HttpStatusCode.SeeOther or HttpStatusCode.TemporaryRedirect or HttpStatusCode.PermanentRedirect)
   {
    var location = response.Headers.Location; response.Dispose();
    if (!asset || location == null) throw new InvalidDataException("허용되지 않은 업데이트 리디렉션입니다.");
    var next = location.IsAbsoluteUri ? location : new Uri(current, location);
    if (!UpdateFeed.IsCdnUri(next) && next != initial) throw new InvalidDataException("공식 GitHub 다운로드 서버 밖의 리디렉션을 거부했습니다.");
    current = next; continue;
   }
   try { response.EnsureSuccessStatusCode(); return response; } catch { response.Dispose(); throw; }
  }
  throw new InvalidDataException("업데이트 리디렉션이 너무 많습니다.");
 }
 private static async Task<byte[]> ReadBoundedAsync(HttpResponseMessage response, int limit, CancellationToken cancellation)
 {
  if (response.Content.Headers.ContentLength > limit) throw new InvalidDataException("업데이트 응답이 너무 큽니다.");
  using var output = new MemoryStream(); await using var input = await response.Content.ReadAsStreamAsync(cancellation);
  var buffer = new byte[8192]; int count;
  while ((count = await input.ReadAsync(buffer, cancellation)) > 0) { if (output.Length + count > limit) throw new InvalidDataException("업데이트 응답이 너무 큽니다."); await output.WriteAsync(buffer.AsMemory(0, count), cancellation); }
  return output.ToArray();
 }
 public async Task<DownloadedUpdate> DownloadAsync(UpdateRelease release, string directory, IProgress<double>? progress, CancellationToken cancellation)
 {
  string name = UpdateFeed.ArchiveName(release.Version);
  if (!UpdateFeed.IsAssetUri(release.Archive, release.Tag, name) || !UpdateFeed.IsAssetUri(release.Checksum, release.Tag, name + ".sha256")) throw new InvalidDataException("공식 릴리스 주소가 아닙니다.");
  using var hashResponse = await GetAsync(release.Checksum, true, cancellation);
  string hash = UpdateFeed.ParseChecksum(Encoding.UTF8.GetString(await ReadBoundedAsync(hashResponse, 4096, cancellation)), name);
  if (release.GitHubDigest != null && !release.GitHubDigest.Equals("sha256:" + hash, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("GitHub 릴리스 해시와 SHA256 파일이 다릅니다.");
  Directory.CreateDirectory(directory);
  string archive = Path.Combine(directory, name), partial = archive + ".partial";
  try
  {
   using var response = await GetAsync(release.Archive, true, cancellation);
   if (response.Content.Headers.ContentLength is long length && length != release.ArchiveBytes) throw new InvalidDataException("다운로드 크기가 릴리스 정보와 다릅니다.");
   await using (var input = await response.Content.ReadAsStreamAsync(cancellation))
   await using (var output = new FileStream(partial, FileMode.CreateNew, FileAccess.Write, FileShare.None, 131072, true))
   using (var digest = IncrementalHash.CreateHash(HashAlgorithmName.SHA256))
   {
    long total = 0; var buffer = new byte[131072]; int count;
    while ((count = await input.ReadAsync(buffer, cancellation)) > 0)
    {
     total += count; if (total > release.ArchiveBytes || total > MaximumArchiveBytes) throw new InvalidDataException("다운로드 크기가 허용 범위를 넘었습니다.");
     digest.AppendData(buffer, 0, count); await output.WriteAsync(buffer.AsMemory(0, count), cancellation); progress?.Report((double)total / release.ArchiveBytes);
    }
    if (total != release.ArchiveBytes || !Convert.ToHexString(digest.GetHashAndReset()).Equals(hash, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("다운로드 SHA256 검증에 실패했습니다. 설치하지 않았습니다.");
   }
   File.Move(partial, archive, true);
   return new(archive, hash, release.Version.ToString());
  }
  finally { if (File.Exists(partial)) File.Delete(partial); }
 }
 public void Dispose() => _client.Dispose();
}

public sealed record DownloadedUpdate(string Archive, string Sha256, string Version);
