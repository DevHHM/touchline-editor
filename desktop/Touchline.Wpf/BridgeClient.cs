using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;

namespace Touchline.Wpf;

public sealed class BridgeException : Exception
{
    public BridgeException(string message) : base(message) { }
    public BridgeException(string message, Exception inner) : base(message, inner) { }
}

/// <summary>Owns one hidden Rust bridge, with one in-flight protocol request.</summary>
public sealed class BridgeClient : IAsyncDisposable
{
    private readonly Process _process;
    private readonly SemaphoreSlim _requestLock = new(1, 1);
    private readonly CancellationTokenSource _lifetime = new();
    private readonly Task _stderrPump;
    private string? _fault;
    private int _disposed;
    private long _nextId;
    private int _uncertainRequest;

    public event Action<string>? Diagnostic;
    // A broken response is not enough to replace a possibly writing worker.
    // Automatic restart is allowed only after its process has actually exited.
    public bool HasExited
    {
        get { try { return _process.HasExited; } catch (InvalidOperationException) { return false; } }
    }

    public BridgeClient(string exePath)
    {
        if (string.IsNullOrWhiteSpace(exePath)) throw new ArgumentException("Rust 실행 파일 경로가 필요합니다.", nameof(exePath));
        exePath = Path.GetFullPath(exePath);
        if (!File.Exists(exePath)) throw new FileNotFoundException("Rust 엔진 실행 파일을 찾을 수 없습니다.", exePath);
        var start = new ProcessStartInfo
        {
            FileName = exePath,
            WorkingDirectory = Path.GetDirectoryName(exePath)!,
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardInputEncoding = new UTF8Encoding(false),
            StandardOutputEncoding = new UTF8Encoding(false),
            StandardErrorEncoding = new UTF8Encoding(false)
        };
        start.ArgumentList.Add("--bridge");
        _process = new Process { StartInfo = start };
        try
        {
            if (!_process.Start()) throw new BridgeException("Rust 엔진을 시작하지 못했습니다.");
            _process.StandardInput.AutoFlush = true;
        }
        catch
        {
            _process.Dispose();
            _lifetime.Dispose();
            throw;
        }
        _stderrPump = PumpStderrAsync();
    }

    public async Task<JsonElement> CallAsync(string command, object? args = null, CancellationToken cancellationToken = default)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(command);
        ThrowIfUnavailable();
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _lifetime.Token);
        await _requestLock.WaitAsync(linked.Token).ConfigureAwait(false);
        try
        {
            ThrowIfUnavailable();
            linked.Token.ThrowIfCancellationRequested();
            var id = Interlocked.Increment(ref _nextId);
            var json = JsonSerializer.Serialize(new { id, command, args = args ?? new { } }, JsonDefaults.Options);
            // Cancellation is honored while queued and before transmission. Once a
            // request starts, always drain its result: killing a transaction worker can
            // interrupt native process-resume cleanup, and hiding a successful write as
            // "cancelled" would leave the UI with an incorrect applied-state assumption.
            Volatile.Write(ref _uncertainRequest, 1);
            await _process.StandardInput.WriteLineAsync(json).ConfigureAwait(false);
            var line = await _process.StandardOutput.ReadLineAsync().ConfigureAwait(false);
            if (line is null) throw new IOException("Rust 엔진 연결이 종료되었습니다.");
            using var document = JsonDocument.Parse(line);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object ||
                !root.TryGetProperty("id", out var responseId) ||
                responseId.ValueKind != JsonValueKind.Number || !responseId.TryGetInt64(out var numericId) || numericId != id ||
                !root.TryGetProperty("ok", out var ok) || ok.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                throw new IOException("Rust 엔진 응답 순서 또는 형식이 올바르지 않습니다.");
            Volatile.Write(ref _uncertainRequest, 0);
            if (!ok.GetBoolean())
            {
                var error = root.TryGetProperty("error", out var errorElement)
                    ? errorElement.ValueKind == JsonValueKind.String ? errorElement.GetString() : errorElement.GetRawText()
                    : "요청을 처리하지 못했습니다.";
                throw new BridgeException(error ?? "요청을 처리하지 못했습니다.");
            }
            return root.TryGetProperty("data", out var data) ? data.Clone() : JsonSerializer.SerializeToElement<object?>(null);
        }
        catch (Exception ex) when (ex is IOException or JsonException or InvalidOperationException)
        {
            FaultAndStop(ex.Message);
            throw new BridgeException("Rust 엔진과 통신하지 못했습니다. " + ex.Message, ex);
        }
        finally
        {
            _requestLock.Release();
        }
    }

    private async Task PumpStderrAsync()
    {
        try
        {
            while (await _process.StandardError.ReadLineAsync(_lifetime.Token).ConfigureAwait(false) is { } line)
            {
                try { Diagnostic?.Invoke(line); }
                catch { /* Diagnostics must never interrupt protocol IO. */ }
            }
        }
        catch (OperationCanceledException) { }
        catch (ObjectDisposedException) { }
        catch (IOException) { }
    }

    private void ThrowIfUnavailable()
    {
        ObjectDisposedException.ThrowIf(Volatile.Read(ref _disposed) != 0, this);
        if (_fault is { } fault) throw new BridgeException(fault);
        if (_process.HasExited) throw new BridgeException($"Rust 엔진이 종료되었습니다. 종료 코드: {_process.ExitCode}");
    }

    private void FaultAndStop(string reason)
    {
        Interlocked.CompareExchange(ref _fault, reason, null);
        _lifetime.Cancel();
        // A broken pipe does not prove that an accepted native transaction finished.
        // Stop accepting work and let stdin EOF end the worker after its current action.
        // Never force-terminate a worker whose transaction outcome is unknown.
        try { _process.StandardInput.Close(); }
        catch (InvalidOperationException) { }
        catch (IOException) { }
    }

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        _lifetime.Cancel();
        await _requestLock.WaitAsync().ConfigureAwait(false);
        try
        {
            try { _process.StandardInput.Close(); } catch (IOException) { }
            try
            {
                using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                await _process.WaitForExitAsync(timeout.Token).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                try
                {
                    if (Volatile.Read(ref _uncertainRequest) == 0 && !_process.HasExited)
                        _process.Kill(entireProcessTree: true);
                }
                catch (InvalidOperationException) { }
                catch (System.ComponentModel.Win32Exception) { }
            }
            try { await _stderrPump.ConfigureAwait(false); } catch (InvalidOperationException) { }
            _process.Dispose();
        }
        finally
        {
            _requestLock.Release();
            // Do not dispose the semaphore while pre-existing queued callers unwind.
            _lifetime.Dispose();
        }
    }
}
