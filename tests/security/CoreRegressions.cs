using System.Collections.Concurrent;
using System.Security;
using Zipelo.Core;

namespace Zipelo.SecurityTests;

internal sealed partial class CoreTests
{
    private static readonly TimeSpan RegressionTimeout = TimeSpan.FromSeconds(5);

    private static async Task ConcurrentPasswords()
    {
        using var firstSecret = TestSecrets.Create();
        using var secondSecret = TestSecrets.Create();
        var response = new TaskCompletionSource<SecureString?>(TaskCreationOptions.RunContinuationsAsynchronously);
        var challenges = new ConcurrentQueue<PasswordChallenge>();
        var activeCounts = new ConcurrentQueue<int>();
        var observed = new ConcurrentQueue<JobEvent>();
        var active = 0;
        var calls = 0;
        var callsWhileWaiting = 0;
        var bothPending = false;
        var waitingPreserved = false;
        var correctFirst = false;
        var correctSecond = false;

        var run = await Execute(Stub(async context =>
        {
            var first = context.UsePasswordAsync((secret, token) =>
            {
                correctFirst = ReferenceEquals(firstSecret, secret);
                return ValueTask.FromResult(0);
            }).AsTask();
            var second = context.UsePasswordAsync((secret, token) =>
            {
                correctSecond = ReferenceEquals(secondSecret, secret);
                return ValueTask.FromResult(0);
            }).AsTask();
            try
            {
                callsWhileWaiting = Volatile.Read(ref calls);
                bothPending = !first.IsCompleted && !second.IsCompleted;
                var eventCount = observed.Count;
                await context.ListEntryAsync(new("listed", "listed.txt", EntryKind.File, 0));
                await Write(context, "written.txt", [1, 2]);
                var snapshot = observed.ToArray();
                waitingPreserved = snapshot[^1].State == JobState.WaitingForPassword &&
                    snapshot.Skip(eventCount).All(value => value.State == JobState.WaitingForPassword);
            }
            finally
            {
                response.TrySetResult(firstSecret);
                await Task.WhenAll(first, second).WaitAsync(RegressionTimeout);
            }
        }), passwords: async (challenge, token) =>
        {
            challenges.Enqueue(challenge);
            activeCounts.Enqueue(Interlocked.Increment(ref active));
            var call = Interlocked.Increment(ref calls);
            try
            {
                return call == 1 ? await response.Task : secondSecret;
            }
            finally { Interlocked.Decrement(ref active); }
        }, observe: observed.Enqueue);

        Check.Equal(JobState.Completed, run.Result.State);
        Check.Equal(1, callsWhileWaiting);
        Check.That(bothPending, "첫 제공자 응답 전에 다른 요청이 완료됨");
        Check.That(waitingPreserved, "항목 진행이 암호 대기 상태를 덮어씀");
        Check.Equal(1, activeCounts.Max());
        Check.Equal(2, calls);
        Check.That(correctFirst && correctSecond, "다른 요청의 암호를 소비함");
        var requests = challenges.ToArray();
        Check.That(requests.Select(value => value.Attempt).SequenceEqual(new[] { 1, 2 }),
            "동시 요청의 시도 번호가 잘못됨");
        Check.Equal(2, requests.Select(value => value.RequestId).Distinct().Count());
        Check.That(requests.All(value => value.JobId == run.Result.JobId), "다른 작업의 암호 요청임");
        Check.Throws<ObjectDisposedException>(() => { _ = firstSecret.Length; });
        Check.Throws<ObjectDisposedException>(() => { _ = secondSecret.Length; });
    }

    private static async Task QueuedPasswordCancellation()
    {
        using var cancellation = new CancellationTokenSource();
        using var lateSecret = TestSecrets.Create();
        var response = new TaskCompletionSource<SecureString?>(TaskCreationOptions.RunContinuationsAsynchronously);
        var observed = new ConcurrentQueue<JobEvent>();
        var calls = 0;
        var consumed = 0;
        var run = await Execute(Stub(async context =>
        {
            ValueTask<int> Consume(SecureString secret, CancellationToken token)
            {
                Interlocked.Increment(ref consumed);
                return ValueTask.FromResult(0);
            }

            var first = context.UsePasswordAsync(Consume).AsTask();
            var second = context.UsePasswordAsync(Consume).AsTask();
            cancellation.Cancel();
            try
            {
                await Check.ThrowsAsync<OperationCanceledException>(
                    () => second.WaitAsync(RegressionTimeout));
            }
            finally
            {
                // 취소를 무시하고 늦게 반환하는 제공자를 재현함.
                response.TrySetResult(lateSecret);
                await Check.ThrowsAsync<OperationCanceledException>(
                    () => first.WaitAsync(RegressionTimeout));
            }
            context.CancellationToken.ThrowIfCancellationRequested();
        }), passwords: async (challenge, token) =>
        {
            Interlocked.Increment(ref calls);
            return await response.Task;
        }, token: cancellation.Token, observe: observed.Enqueue);

        Cancelled(run);
        Check.Equal(1, calls);
        Check.Equal(0, consumed);
        Check.Throws<ObjectDisposedException>(() => { _ = lateSecret.Length; });
        var events = observed.ToArray();
        var cancelIndex = Array.FindIndex(events, value => value.State == JobState.Cancelling);
        Check.That(cancelIndex >= 0, "취소 요청 이벤트가 없음");
        Check.That(events.Skip(cancelIndex).All(value =>
            value.State is JobState.Cancelling or JobState.Cancelled), "취소 뒤 활성 상태가 보고됨");
        var count = observed.Count;
        cancellation.Cancel();
        Check.Equal(count, observed.Count);
        Check.Equal(JobState.Cancelled, events[^1].State);
    }

    private static async Task PasswordGateAfterFailures()
    {
        var secrets = new List<SecureString>();
        var challenges = new List<PasswordChallenge>();
        var run = await Execute(Stub(async context =>
        {
            await Check.ThrowsAsync<IOException>(() => context.UsePasswordAsync(
                (secret, token) => ValueTask.FromResult(0)).AsTask().WaitAsync(RegressionTimeout));
            await Check.ThrowsAsync<IOException>(() => context.UsePasswordAsync<int>(
                (secret, token) => ValueTask.FromException<int>(new IOException("테스트 소비 실패")))
                .AsTask().WaitAsync(RegressionTimeout));
            var length = await context.UsePasswordAsync(
                (secret, token) => ValueTask.FromResult(secret.Length)).AsTask().WaitAsync(RegressionTimeout);
            Check.That(length > 0, "실패 후 암호 재요청에 실패함");
        }), passwords: (challenge, token) =>
        {
            challenges.Add(challenge);
            if (challenges.Count == 1)
            {
                return ValueTask.FromException<SecureString?>(new IOException("테스트 제공 실패"));
            }
            var secret = TestSecrets.Create();
            secrets.Add(secret);
            return ValueTask.FromResult<SecureString?>(secret);
        });

        Check.Equal(JobState.Completed, run.Result.State);
        Check.That(challenges.Select(value => value.Attempt).SequenceEqual(new[] { 1, 2, 3 }),
            "재요청의 시도 번호가 잘못됨");
        Check.Equal(2, secrets.Count);
        foreach (var secret in secrets)
        {
            Check.Throws<ObjectDisposedException>(() => { _ = secret.Length; });
        }
    }

    private static async Task PasswordConsumerReentry()
    {
        var secrets = new List<SecureString>();
        var calls = 0;
        var run = await Execute(Stub(async context =>
        {
            var combined = await context.UsePasswordAsync(async (outer, token) =>
            {
                var length = outer.Length;
                var innerLength = await context.UsePasswordAsync(
                    (inner, innerToken) => ValueTask.FromResult(inner.Length));
                Check.Equal(length, outer.Length);
                return length + innerLength;
            }).AsTask().WaitAsync(RegressionTimeout);
            Check.That(combined > 0, "소비자의 암호 재요청이 완료되지 않음");
        }), passwords: (challenge, token) =>
        {
            calls++;
            var secret = TestSecrets.Create();
            secrets.Add(secret);
            return ValueTask.FromResult<SecureString?>(secret);
        });

        Check.Equal(JobState.Completed, run.Result.State);
        Check.Equal(2, calls);
        foreach (var secret in secrets)
        {
            Check.Throws<ObjectDisposedException>(() => { _ = secret.Length; });
        }
    }

    private static async Task InitializationCleanup()
    {
        foreach (var scenario in new[] { "null_limits", "invalid_limits", "input_copy" })
        {
            using var input = new MemoryStream(new byte[] { 1 }, false);
            var output = new RecordingOutput();
            var engine = Stub(_ => Task.CompletedTask);
            var events = new List<JobEvent>();
            var limits = scenario == "null_limits" ? null :
                scenario == "invalid_limits" ? Limits with { MaxEntries = 0 } : Limits;
            IReadOnlyList<Stream> inputs = scenario == "input_copy" ? new ThrowingInputs() : new[] { input };
            var result = await new ArchiveJob(Guid.NewGuid(), limits!).ExecuteAsync(
                ArchiveAction.List, "zip", engine, inputs, output, TestSecrets.None,
                new InlineProgress(events.Add), default);

            Check.Equal(JobState.Failed, result.State);
            Check.Equal(scenario == "input_copy" ? ArchiveError.EngineFailure : ArchiveError.UnsupportedOperation,
                result.Error);
            Check.Equal(0, engine.Calls);
            Check.Equal(1, output.DisposeCalls);
            Check.That(output.Disposed, "초기화 실패 뒤 출력 정리가 누락됨");
            Check.Equal(0, output.CreateCalls);
            Check.Equal(0, output.CommitCalls);
            Check.Equal(0, result.Entries.Count);
            Check.Equal(1, events.Count);
            Check.Equal(JobState.Failed, events[0].State);
            Check.Equal(0L, events[0].ProcessedBytes);
            Check.That(input.CanRead, "초기화 실패가 호출자 입력을 폐기함");
        }
    }

    private static async Task RejectedOutputOwnership()
    {
        using var input = new MemoryStream(new byte[] { 1 }, false);
        var job = new ArchiveJob(Guid.NewGuid(), Limits);
        var engine = Stub(_ => Task.CompletedTask);
        var rejected = new RecordingOutput();
        try
        {
            await Check.ThrowsAsync<ArgumentNullException>(async () => await job.ExecuteAsync(
                ArchiveAction.List, "zip", null!, [input], rejected, TestSecrets.None, null, default));
            Check.Equal(0, rejected.DisposeCalls);
        }
        finally { await rejected.DisposeAsync(); }

        var accepted = new RecordingOutput();
        var result = await job.ExecuteAsync(ArchiveAction.List, "zip", engine, [input],
            accepted, TestSecrets.None, null, default);
        Check.Equal(JobState.Completed, result.State);
        Check.Equal(1, accepted.DisposeCalls);

        var reused = new RecordingOutput();
        try
        {
            await Check.ThrowsAsync<InvalidOperationException>(async () => await job.ExecuteAsync(
                ArchiveAction.List, "zip", engine, [input], reused, TestSecrets.None, null, default));
            Check.Equal(0, reused.DisposeCalls);
        }
        finally { await reused.DisposeAsync(); }
    }

    private static async Task IntegrityCleanupFailure()
    {
        var run = await Execute(Stub(context => Write(context, "bad.txt", [1],
            verification: VerificationState.Failed)), output: new RecordingOutput { FailDispose = true });
        Failed(run, ArchiveError.IntegrityFailure);
        Check.Equal(1, run.Output.DisposeCalls);
        Check.Equal(VerificationState.Failed, run.Result.Entries.Single().Verification);
        Check.Equal(ArchiveError.IntegrityFailure, run.Result.Entries.Single().Error);
    }

    private static async Task CancellationCleanupFailure()
    {
        using var cancellation = new CancellationTokenSource();
        var run = await Execute(Stub(context =>
        {
            cancellation.Cancel();
            context.CancellationToken.ThrowIfCancellationRequested();
            return Task.CompletedTask;
        }), output: new RecordingOutput { FailDispose = true }, token: cancellation.Token);
        Failed(run, ArchiveError.Cancelled);
        Check.Equal(1, run.Output.DisposeCalls);
        Check.That(!run.Events.Any(value => value.State == JobState.Cancelled),
            "정리 실패를 정상 취소 완료로 표시함");
    }

    private static async Task LateCommitCancellation()
    {
        foreach (var failCleanup in new[] { false, true })
        {
            using var cancellation = new CancellationTokenSource();
            var output = new RecordingOutput
            {
                AfterCommit = cancellation.Cancel,
                FailDispose = failCleanup
            };
            var run = await Execute(Stub(context => Write(context, "published.txt", [1])),
                output: output, token: cancellation.Token);
            Check.Equal(failCleanup ? JobState.Failed : JobState.Completed, run.Result.State);
            Check.Equal(failCleanup ? ArchiveError.OutputUnavailable : ArchiveError.None, run.Result.Error);
            Check.Equal(1, output.CommitCalls);
            Check.Equal(1, output.DisposeCalls);
            Check.Equal(1, output.Published.Count);
            var entry = run.Result.Entries.Single();
            Check.Equal(EntryOutcome.Extracted, entry.Outcome);
            Check.Equal(ArchiveError.None, entry.Error);
            Check.That(output.Published["published.txt"].SequenceEqual(new byte[] { 1 }),
                "늦은 취소가 확정된 결과를 바꿈");
        }
    }

    private static async Task DeferredCleanup()
    {
        var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var observed = new ConcurrentQueue<JobEvent>();
        var output = new RecordingOutput
        {
            BeforeDispose = async () =>
            {
                entered.TrySetResult();
                await release.Task;
            }
        };
        var execution = Execute(Stub(_ => Task.CompletedTask), output: output, observe: observed.Enqueue);
        var completedTooEarly = false;
        var terminalTooEarly = false;
        try
        {
            await entered.Task.WaitAsync(RegressionTimeout);
            completedTooEarly = execution.IsCompleted;
            terminalTooEarly = observed.Any(value => value.State is
                JobState.Completed or JobState.Partial or JobState.Cancelled or JobState.Failed);
        }
        finally { release.TrySetResult(); }

        var run = await execution.WaitAsync(RegressionTimeout);
        Check.That(!completedTooEarly, "출력 정리 전에 작업이 완료됨");
        Check.That(!terminalTooEarly, "출력 정리 전에 종료 이벤트가 보고됨");
        Check.Equal(JobState.Completed, run.Result.State);
        Check.Equal(1, output.DisposeCalls);
        Check.Equal(JobState.Completed, observed.Last().State);
    }

    private sealed class ThrowingInputs : IReadOnlyList<Stream>
    {
        public int Count => 1;
        public Stream this[int index] => throw new IOException("테스트 입력 복사 실패");
        public IEnumerator<Stream> GetEnumerator() => throw new IOException("테스트 입력 복사 실패");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
}
