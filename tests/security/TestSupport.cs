using System.Security;
using Zipelo.Core;

namespace Zipelo.SecurityTests;

internal static class Check
{
    public static void That(bool condition, string message)
    {
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }

    public static void Equal<T>(T expected, T actual)
    {
        That(EqualityComparer<T>.Default.Equals(expected, actual), "기대 결과와 실제 결과가 다름");
    }

    public static T Throws<T>(Action action) where T : Exception
    {
        try
        {
            action();
        }
        catch (T exception)
        {
            return exception;
        }

        throw new InvalidOperationException("예상한 예외가 발생하지 않음");
    }

    public static async Task<T> ThrowsAsync<T>(Func<Task> action) where T : Exception
    {
        try
        {
            await action();
        }
        catch (T exception)
        {
            return exception;
        }

        throw new InvalidOperationException("예상한 비동기 예외가 발생하지 않음");
    }
}

// 실제 압축 엔진을 대체하는 제품 코드가 아닌 코어 호출용 테스트 대역임.
internal sealed class EngineStub(
    Func<IReadOnlyList<Stream>, EngineContext, CancellationToken, Task> execute) : IArchiveEngine
{
    public int Calls { get; private set; }

    public IReadOnlyCollection<EngineCapability> Capabilities { get; } = Enum
        .GetValues<ArchiveAction>()
        .Select(action => new EngineCapability("zip", action))
        .ToArray();

    public Task ExecuteAsync(
        ArchiveAction action,
        IReadOnlyList<Stream> inputs,
        EngineContext context,
        CancellationToken cancellationToken)
    {
        Calls++;
        return execute(inputs, context, cancellationToken);
    }
}

// 메모리에서 게시 여부만 관찰함. 파일시스템의 링크 방어를 구현하거나 입증하지 않음.
internal sealed class RecordingOutput : IOutputTransaction
{
    public Dictionary<string, MemoryStream> Staged { get; } = new(StringComparer.Ordinal);
    public Dictionary<string, byte[]> Published { get; } = new(StringComparer.Ordinal);
    public ArchiveError? CreateError { get; init; }
    public bool FailDispose { get; init; }
    public bool Disposed { get; private set; }
    public int CreateCalls { get; private set; }
    public int CommitCalls { get; private set; }
    public int DisposeCalls { get; private set; }
    public Action? BeforeCommit { get; init; }
    public Action? AfterCommit { get; init; }
    public Func<ValueTask>? BeforeDispose { get; init; }

    public ValueTask<Stream> CreateFileAsync(
        IReadOnlyList<string> relativeSegments,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        CreateCalls++;
        if (CreateError is { } error)
        {
            throw new ArchiveException(error);
        }

        var stream = new MemoryStream();
        Staged.Add(string.Join('/', relativeSegments), stream);
        return ValueTask.FromResult<Stream>(stream);
    }

    public ValueTask CommitAsync(CancellationToken cancellationToken)
    {
        BeforeCommit?.Invoke();
        cancellationToken.ThrowIfCancellationRequested();
        CommitCalls++;
        foreach (var (name, stream) in Staged)
        {
            Published.Add(name, stream.ToArray());
        }

        AfterCommit?.Invoke();
        return ValueTask.CompletedTask;
    }

    public async ValueTask DisposeAsync()
    {
        DisposeCalls++;
        if (BeforeDispose is not null) { await BeforeDispose(); }
        Disposed = true;
        foreach (var stream in Staged.Values)
        {
            stream.Dispose();
        }

        if (FailDispose) { throw new IOException("테스트 정리 실패"); }
    }
}

internal sealed class InlineProgress(Action<JobEvent> report) : IProgress<JobEvent>
{
    public void Report(JobEvent value) => report(value);
}

internal sealed class ManualTimeProvider : TimeProvider
{
    private long timestamp;

    public override long TimestampFrequency => TimeSpan.TicksPerSecond;
    public override long GetTimestamp() => timestamp;
    public void Advance(TimeSpan duration) => timestamp += duration.Ticks;
}

internal sealed class ChunkedStream(byte[] bytes, Action? afterRead = null) : MemoryStream(bytes, false)
{
    public override ValueTask<int> ReadAsync(
        Memory<byte> buffer,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var count = Read(buffer.Span[..Math.Min(buffer.Length, 1)]);
        if (count != 0)
        {
            afterRead?.Invoke();
        }

        return ValueTask.FromResult(count);
    }
}

internal static class TestSecrets
{
    public static SecureString Create()
    {
        var value = new SecureString();
        foreach (var character in "테스트 암호! 123")
        {
            value.AppendChar(character);
        }

        value.MakeReadOnly();
        return value;
    }

    public static ValueTask<SecureString?> None(
        PasswordChallenge challenge,
        CancellationToken cancellationToken) => ValueTask.FromResult<SecureString?>(null);
}
