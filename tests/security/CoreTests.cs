using System.IO.Compression;
using System.Security;
using System.Security.Cryptography;
using Zipelo.Core;

namespace Zipelo.SecurityTests;

internal sealed partial class CoreTests(string root, string sandbox)
{
    private static JobLimits Limits => new(32, 1024, 1024, TimeSpan.FromSeconds(5), 3);

    public IEnumerable<(string Name, Func<Task> Run)> Cases()
    {
        yield return ("path.accept_relative_unicode", () => Sync(ValidPaths));
        yield return ("path.reject_traversal_and_windows_aliases", () => Sync(UnsafePaths));
        yield return ("budget.exact_boundary_and_latched_exhaustion", () => Sync(BudgetBoundaries));
        yield return ("budget.reject_negative_and_overflow", () => Sync(BudgetInvalidCharges));
        yield return ("budget.deadline_with_controlled_clock", () => Sync(BudgetDeadline));
        yield return ("budget.concurrent_cumulative_charges", ConcurrentBudget);
        yield return ("job.reject_writable_input", WritableInput);
        yield return ("job.unavailable_engine_fails", UnavailableEngineFails);
        yield return ("job.unavailable_output_fails", UnavailableOutputFails);
        yield return ("job.single_use", SingleUse);
        yield return ("output.block_path_before_create", UnsafeWrite);
        yield return ("output.reject_archive_links", ArchiveLinks);
        yield return ("output.reject_parent_collisions", OutputCollisions);
        yield return ("output.propagate_existing_file_conflict", ExistingConflict);
        yield return ("output.verification_failure_rolls_back", VerificationFailure);
        yield return ("output.unavailable_verification_remains_unavailable", VerificationUnavailable);
        yield return ("budget.count_actual_bytes_not_header", ActualBytes);
        yield return ("budget.accumulate_entries_and_intermediate_work", IntermediateBudget);
        yield return ("budget.swallowed_failure_cannot_commit", SwallowedFailure);
        yield return ("cancel.before_engine_starts", PreCancelled);
        yield return ("cancel.during_copy_prevents_publish", CancelDuringCopy);
        yield return ("cancel.before_commit_prevents_publish", CancelBeforeCommit);
        yield return ("job.observer_failure_does_not_skip_cleanup", ObserverFailure);
        yield return ("job.cleanup_failure_is_not_success", CleanupFailure);
        yield return ("password.cancel_without_reprompt", PasswordCancelled);
        yield return ("password.disposed_after_success_and_failure", PasswordDisposed);
        yield return ("password.attempt_limit_and_request_ids", PasswordAttempts);
        yield return ("preview.cannot_use_extraction_output", PreviewRejectsWrite);
        yield return ("fixtures.core_policy_and_original_preservation", ArchiveFixtures);
        yield return ("password.concurrent_provider_and_waiting_state", ConcurrentPasswords);
        yield return ("password.queued_cancel_discards_late_secret", QueuedPasswordCancellation);
        yield return ("password.gate_released_after_failures", PasswordGateAfterFailures);
        yield return ("password.consumer_can_request_again", PasswordConsumerReentry);
        yield return ("job.initialization_failure_cleans_once", InitializationCleanup);
        yield return ("job.precondition_rejection_keeps_output_ownership", RejectedOutputOwnership);
        yield return ("job.cleanup_preserves_integrity_error", IntegrityCleanupFailure);
        yield return ("job.cleanup_after_cancel_is_failed", CancellationCleanupFailure);
        yield return ("output.late_cancel_preserves_published_result", LateCommitCancellation);
        yield return ("job.terminal_event_waits_for_cleanup", DeferredCleanup);
        yield return ("path.windows_and_linux_policy", () => Sync(PlatformNames));
        yield return ("output.case_distinction_by_os", PlatformCaseOutputs);
    }

    private static Task Sync(Action check)
    {
        check();
        return Task.CompletedTask;
    }

    private static void ValidPaths()
    {
        foreach (var name in new[] { "hello.txt", "한글/빈파일.txt", "a\\b.txt", ".hidden", "COM10.txt" })
        {
            var segments = OutputPathPolicy.Validate(name);
            Check.Equal(name.Replace('\\', '/'), string.Join('/', segments));
        }
    }

    private static void UnsafePaths()
    {
        string[] names = [
            "", " ", "../escape.txt", "..\\escape.txt", "a/../../escape.txt",
            "/absolute.txt", "\\absolute.txt", "C:\\escape.txt", "C:escape.txt",
            "\\\\server\\share\\escape.txt", "\\\\?\\C:\\escape.txt", "\\\\.\\NUL",
            "file:stream", "file::$DATA", "a//b", "./file", "folder/", "a/./b",
            "name.", "name ", "NUL", "nul.txt", "CON .txt", "AUX", "PRN",
            "CONIN$", "CONOUT$", "com1.txt", "LPT9", "COM¹", "LPT².txt",
            "name\0.txt", "line\nfeed", "a<b", "a>b", "a\"b", "a|b", "a?b", "a*b",
            "e\u0301.txt", new string('a', 256), new string('a', 4097)
        ];
        foreach (var name in names)
        {
            var error = Check.Throws<ArchiveException>(() => OutputPathPolicy.Validate(name));
            Check.Equal(ArchiveError.UnsafePath, error.Code);
        }
    }

    private static void BudgetBoundaries()
    {
        var budget = new JobBudget(Limits with { MaxEntries = 1, MaxProcessedBytes = 4, MaxOutputBytes = 3 });
        budget.AddEntry(default);
        budget.ChargeProcessed(3, default);
        budget.ChargeProcessed(1, default);
        budget.ReserveOutput(3, default);
        Check.Equal(new BudgetUsage(1, 4, 3), budget.Usage);
        Check.Equal(ArchiveError.BudgetExceeded,
            Check.Throws<ArchiveException>(() => budget.ChargeProcessed(1, default)).Code);
        Check.Equal(new BudgetUsage(1, 4, 3), budget.Usage);
        Check.Throws<ArchiveException>(() => budget.ChargeProcessed(0, default));

        var entries = new JobBudget(Limits with { MaxEntries = 1 });
        entries.AddEntry(default);
        Check.Throws<ArchiveException>(() => entries.AddEntry(default));
        var output = new JobBudget(Limits with { MaxOutputBytes = 1 });
        output.ReserveOutput(1, default);
        Check.Throws<ArchiveException>(() => output.ReserveOutput(1, default));
    }

    private static void BudgetInvalidCharges()
    {
        Check.Throws<ArgumentOutOfRangeException>(() => new JobBudget(Limits with { MaxEntries = 0 }));
        var budget = new JobBudget(Limits with { MaxProcessedBytes = long.MaxValue });
        Check.Throws<ArgumentOutOfRangeException>(() => budget.ChargeProcessed(-1, default));
        Check.Throws<ArgumentOutOfRangeException>(() => budget.ReserveOutput(-1, default));
        Check.Equal(new BudgetUsage(0, 0, 0), budget.Usage);
        budget.ChargeProcessed(long.MaxValue, default);
        Check.Throws<ArchiveException>(() => budget.ChargeProcessed(1, default));
        Check.Equal(long.MaxValue, budget.Usage.ProcessedBytes);
    }

    private static void BudgetDeadline()
    {
        var clock = new ManualTimeProvider();
        var budget = new JobBudget(Limits with { MaxDuration = TimeSpan.FromSeconds(1) }, clock);
        clock.Advance(TimeSpan.FromTicks(TimeSpan.TicksPerSecond - 1));
        budget.Check(default);
        clock.Advance(TimeSpan.FromTicks(1));
        Check.Throws<ArchiveException>(() => budget.Check(default));
    }

    private static async Task ConcurrentBudget()
    {
        var budget = new JobBudget(Limits with { MaxProcessedBytes = 8 });
        var accepted = 0;
        var rejected = 0;
        await Task.WhenAll(Enumerable.Range(0, 16).Select(_ => Task.Run(() =>
        {
            try
            {
                budget.ChargeProcessed(1, default);
                Interlocked.Increment(ref accepted);
            }
            catch (ArchiveException error) when (error.Code == ArchiveError.BudgetExceeded)
            {
                Interlocked.Increment(ref rejected);
            }
        })));
        Check.Equal(8, accepted);
        Check.Equal(8, rejected);
        Check.Equal(8L, budget.Usage.ProcessedBytes);
    }

    private static async Task WritableInput()
    {
        using var input = new MemoryStream(new byte[] { 1, 2, 3 }, true);
        var engine = Stub(_ => Task.CompletedTask);
        var output = new RecordingOutput();
        var result = await new ArchiveJob(Guid.NewGuid(), Limits).ExecuteAsync(
            ArchiveAction.Extract, "zip", engine, [input], output, TestSecrets.None, null, default);
        Check.Equal(ArchiveError.UnsupportedOperation, result.Error);
        Check.Equal(0, engine.Calls);
        Check.That(output.Disposed, "출력 정리가 누락됨");
        Check.That(input.ToArray().SequenceEqual(new byte[] { 1, 2, 3 }), "입력이 변경됨");
    }

    private static async Task UnavailableEngineFails()
    {
        var run = await Execute(new UnavailableEngine());
        Failed(run, ArchiveError.EngineUnavailable);
    }

    private static async Task UnavailableOutputFails()
    {
        using var input = new MemoryStream(new byte[] { 1 }, false);
        var result = await new ArchiveJob(Guid.NewGuid(), Limits).ExecuteAsync(
            ArchiveAction.Extract, "zip", Stub(context => Write(context, "a.txt", [1])),
            [input], new UnavailableOutput(), TestSecrets.None, null, default);
        Check.Equal(JobState.Failed, result.State);
        Check.Equal(ArchiveError.OutputUnavailable, result.Error);
    }

    private static async Task SingleUse()
    {
        var job = new ArchiveJob(Guid.NewGuid(), Limits);
        using var input = new MemoryStream(new byte[] { 1 }, false);
        var engine = Stub(_ => Task.CompletedTask);
        await job.ExecuteAsync(ArchiveAction.List, "zip", engine, [input],
            new RecordingOutput(), TestSecrets.None, null, default);
        await Check.ThrowsAsync<InvalidOperationException>(async () =>
            await job.ExecuteAsync(ArchiveAction.List, "zip", engine, [input],
                new RecordingOutput(), TestSecrets.None, null, default));
        Check.Equal(1, engine.Calls);
    }

    private static async Task UnsafeWrite()
    {
        var run = await Execute(Stub(context => Write(context, "../escape.txt", [1])));
        Failed(run, ArchiveError.UnsafePath);
        Check.Equal(0, run.Output.CreateCalls);
    }

    private static async Task ArchiveLinks()
    {
        foreach (var kind in new[] { EntryKind.SymbolicLink, EntryKind.HardLink, EntryKind.Other })
        {
            var run = await Execute(Stub(context => Write(context, "link", [1], kind: kind)));
            Failed(run, ArchiveError.UnsupportedEntry);
            Check.Equal(0, run.Output.CreateCalls);
        }
    }

    private static async Task OutputCollisions()
    {
        foreach (var pair in new[]
        {
            ("file", "file/child"), ("file/child", "file")
        })
        {
            var run = await Execute(Stub(async context =>
            {
                await Write(context, pair.Item1, [1]);
                await Write(context, pair.Item2, [2]);
            }));
            Failed(run, ArchiveError.OutputConflict);
            Check.Equal(1, run.Output.CreateCalls);
            Check.That(run.Result.Entries.All(entry => entry.Outcome == EntryOutcome.Failed),
                "충돌 후 임시 출력을 성공으로 표시함");
        }
    }

    private static async Task ExistingConflict()
    {
        var output = new RecordingOutput { CreateError = ArchiveError.OutputConflict };
        var run = await Execute(Stub(context => Write(context, "existing.txt", [1])), output: output);
        Failed(run, ArchiveError.OutputConflict);
    }

    private static async Task VerificationFailure()
    {
        var run = await Execute(Stub(async context =>
        {
            await Write(context, "good.txt", [1]);
            await Write(context, "bad.txt", [2], verification: VerificationState.Failed);
        }));
        Failed(run, ArchiveError.IntegrityFailure);
        Check.Equal(VerificationState.Failed, run.Result.Entries[1].Verification);
        Check.That(run.Result.Entries.All(entry => entry.Outcome == EntryOutcome.Failed),
            "검증 실패 후 게시하지 않은 출력을 성공으로 표시함");
    }

    private static async Task VerificationUnavailable()
    {
        var run = await Execute(Stub(context => Write(context, "file.txt", [1],
            verification: VerificationState.Unavailable)));
        Check.Equal(JobState.Completed, run.Result.State);
        Check.Equal(EntryOutcome.Extracted, run.Result.Entries.Single().Outcome);
        Check.Equal(VerificationState.Unavailable, run.Result.Entries.Single().Verification);
        Check.Equal(1, run.Output.CommitCalls);
    }

    private static async Task ActualBytes()
    {
        var run = await Execute(Stub(async context =>
        {
            using var stream = new ChunkedStream([1, 2, 3, 4, 5]);
            await context.WriteEntryAsync(new("one", "tiny.txt", EntryKind.File, 0), stream,
                _ => ValueTask.FromResult(VerificationState.Passed));
        }), limits: Limits with { MaxProcessedBytes = 4, MaxOutputBytes = 4 });
        Failed(run, ArchiveError.BudgetExceeded);
        Check.Equal(4L, run.Events[^1].ProcessedBytes);
        Check.Equal(4, run.Output.Staged["tiny.txt"].ToArray().Length);
    }

    private static async Task IntermediateBudget()
    {
        var run = await Execute(Stub(async context =>
        {
            context.Budget.ChargeProcessed(2, context.CancellationToken);
            await Write(context, "one.txt", [1, 2]);
            await Write(context, "two.txt", [3, 4]);
        }), limits: Limits with { MaxProcessedBytes = 5 });
        Failed(run, ArchiveError.BudgetExceeded);

        var entries = await Execute(Stub(async context =>
        {
            await context.ListEntryAsync(new("one", "one", EntryKind.File, 0));
            await context.ListEntryAsync(new("two", "two", EntryKind.File, 0));
        }), limits: Limits with { MaxEntries = 1 }, action: ArchiveAction.List);
        Failed(entries, ArchiveError.BudgetExceeded);
    }

    private static async Task SwallowedFailure()
    {
        var run = await Execute(Stub(async context =>
        {
            await Write(context, "good.txt", [1]);
            try
            {
                await Write(context, "../escape.txt", [2]);
            }
            catch (ArchiveException)
            {
                // 어댑터가 오류를 삼켜도 앞선 출력을 게시하지 않아야 함.
            }
        }));
        Failed(run, ArchiveError.UnsafePath);
    }

    private static async Task PreCancelled()
    {
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        var engine = Stub(_ => Task.CompletedTask);
        var run = await Execute(engine, token: cancellation.Token);
        Cancelled(run);
        Check.Equal(0, engine.Calls);
    }

    private static async Task CancelDuringCopy()
    {
        using var cancellation = new CancellationTokenSource();
        var run = await Execute(Stub(async context =>
        {
            using var stream = new ChunkedStream([1, 2, 3], cancellation.Cancel);
            await context.WriteEntryAsync(new("one", "file.txt", EntryKind.File, 3), stream,
                _ => ValueTask.FromResult(VerificationState.Passed));
        }), token: cancellation.Token);
        Cancelled(run);
        Check.Equal(0, run.Output.Staged["file.txt"].ToArray().Length);
    }

    private static async Task CancelBeforeCommit()
    {
        using var cancellation = new CancellationTokenSource();
        var output = new RecordingOutput { BeforeCommit = cancellation.Cancel };
        var run = await Execute(Stub(context => Write(context, "file.txt", [1])),
            output: output, token: cancellation.Token);
        Cancelled(run);
    }

    private static async Task ObserverFailure()
    {
        using var input = new MemoryStream(new byte[] { 1 }, false);
        var output = new RecordingOutput();
        var result = await new ArchiveJob(Guid.NewGuid(), Limits).ExecuteAsync(
            ArchiveAction.Extract, "zip", Stub(context => Write(context, "file.txt", [1])),
            [input], output, TestSecrets.None,
            new InlineProgress(_ => throw new InvalidOperationException("테스트 관찰자 오류")), default);
        Check.Equal(JobState.Completed, result.State);
        Check.That(output.Disposed, "관찰자 실패로 출력 정리가 누락됨");
    }

    private static async Task CleanupFailure()
    {
        var run = await Execute(Stub(_ => Task.CompletedTask),
            output: new RecordingOutput { FailDispose = true });
        Failed(run, ArchiveError.OutputUnavailable);
    }

    private static async Task PasswordCancelled()
    {
        var requests = 0;
        var consumed = false;
        var run = await Execute(Stub(async context =>
        {
            await context.UsePasswordAsync((secret, token) =>
            {
                consumed = true;
                return ValueTask.FromResult(0);
            });
        }), passwords: (challenge, token) =>
        {
            requests++;
            return ValueTask.FromResult<SecureString?>(null);
        });
        Cancelled(run);
        Check.Equal(1, requests);
        Check.That(!consumed, "취소된 암호를 소비함");
        Check.That(run.Events.Any(value => value.State == JobState.WaitingForPassword), "암호 대기 상태가 없음");
    }

    private static async Task PasswordDisposed()
    {
        foreach (var fail in new[] { false, true })
        {
            var secret = TestSecrets.Create();
            var expectedLength = secret.Length;
            var run = await Execute(Stub(async context =>
            {
                await context.UsePasswordAsync((value, token) =>
                {
                    Check.Equal(expectedLength, value.Length);
                    return fail
                        ? ValueTask.FromException<int>(new IOException("테스트 암호 소비 실패"))
                        : ValueTask.FromResult(value.Length);
                });
            }), passwords: (challenge, token) => ValueTask.FromResult<SecureString?>(secret));
            Check.Equal(fail ? JobState.Failed : JobState.Completed, run.Result.State);
            Check.Throws<ObjectDisposedException>(() => { _ = secret.Length; });
        }
    }

    private static async Task PasswordAttempts()
    {
        var challenges = new List<PasswordChallenge>();
        var run = await Execute(Stub(async context =>
        {
            for (var index = 0; index < 3; index++)
            {
                await context.UsePasswordAsync((secret, token) => ValueTask.FromResult(secret.Length));
            }
        }), limits: Limits with { MaxPasswordAttempts = 2 }, passwords: (challenge, token) =>
        {
            challenges.Add(challenge);
            return ValueTask.FromResult<SecureString?>(TestSecrets.Create());
        });
        Failed(run, ArchiveError.PasswordAttemptsExceeded);
        Check.Equal(2, challenges.Count);
        Check.Equal(1, challenges[0].Attempt);
        Check.Equal(2, challenges[1].Attempt);
        Check.That(challenges.All(value => value.JobId == run.Result.JobId), "다른 작업에 암호를 요청함");
        Check.Equal(2, challenges.Select(value => value.RequestId).Distinct().Count());
    }

    private static async Task PreviewRejectsWrite()
    {
        var run = await Execute(Stub(context => Write(context, "preview.html", [1])),
            action: ArchiveAction.Preview);
        Failed(run, ArchiveError.UnsupportedOperation);
        Check.Equal(0, run.Output.CreateCalls);
    }

    private async Task ArchiveFixtures()
    {
        var scenarios = new (string File, ArchiveError Error)[]
        {
            ("normal.zip", ArchiveError.None),
            ("traversal.zip", ArchiveError.UnsafePath),
            ("backslash-traversal.zip", OperatingSystem.IsWindows() ? ArchiveError.UnsafePath : ArchiveError.None),
            ("case-collision.zip", OperatingSystem.IsWindows() ? ArchiveError.OutputConflict : ArchiveError.None),
            ("parent-collision.zip", ArchiveError.OutputConflict),
            ("symlink.zip", ArchiveError.UnsupportedEntry),
            ("truncated.zip", ArchiveError.EngineFailure)
        };
        foreach (var (name, expected) in scenarios)
        {
            var folder = Path.Combine(sandbox, Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(folder);
            var source = Path.Combine(folder, "source.zip");
            File.Copy(Path.Combine(root, "tests", "security", "fixtures", name), source);
            var sentinel = Path.Combine(folder, "sentinel.txt");
            await File.WriteAllTextAsync(sentinel, "원본 보존 검사");
            var before = SHA256.HashData(await File.ReadAllBytesAsync(source));
            var output = new RecordingOutput();
            var engine = new EngineStub(async (inputs, context, token) =>
            {
                // BCL ZIP 판독은 테스트 입력 공급용임. 실제 엔진의 지원 또는 CRC 검증으로 계산하지 않음.
                using var zip = new ZipArchive(inputs[0], ZipArchiveMode.Read, true);
                var index = 0;
                foreach (var entry in zip.Entries)
                {
                    var kind = ((entry.ExternalAttributes >> 16) & 0xF000) == 0xA000
                        ? EntryKind.SymbolicLink : EntryKind.File;
                    using var decoded = entry.Open();
                    await context.WriteEntryAsync(new((index++).ToString(), entry.FullName, kind, entry.Length),
                        decoded, _ => ValueTask.FromResult(VerificationState.Unavailable));
                }
            });
            await using (var input = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                var result = await new ArchiveJob(Guid.NewGuid(), Limits).ExecuteAsync(
                    ArchiveAction.Extract, "zip", engine, [input], output, TestSecrets.None, null, default);
                Check.Equal(expected, result.Error);
                Check.Equal(expected == ArchiveError.None ? JobState.Completed : JobState.Failed, result.State);
                Check.That(input.CanRead && !input.CanWrite, "입력 수명 또는 읽기 전용 계약이 깨짐");
            }

            var after = SHA256.HashData(await File.ReadAllBytesAsync(source));
            Check.That(before.SequenceEqual(after), "원본 해시가 변경됨");
            Check.Equal("원본 보존 검사", await File.ReadAllTextAsync(sentinel));
            Check.Equal(2, Directory.GetFileSystemEntries(folder).Length);
            Check.That(output.Disposed, "샘플 처리 후 정리가 누락됨");
            if (expected == ArchiveError.None)
            {
                if (name == "normal.zip")
                {
                    Check.Equal(2, output.Published.Count);
                    Check.That(output.Published["hello.txt"].SequenceEqual("zipelo-safe-test-payload\n"u8.ToArray()),
                        "정상 출력이 원문과 다름");
                    Check.Equal(0, output.Published["한글/빈파일.txt"].Length);
                }
                else if (name == "case-collision.zip")
                {
                    Check.Equal(2, output.Published.Count);
                    Check.That(output.Published["Readme.txt"].SequenceEqual("first"u8.ToArray()), "첫 항목이 변경됨");
                    Check.That(output.Published["README.TXT"].SequenceEqual("second"u8.ToArray()), "둘째 항목이 변경됨");
                }
                else
                {
                    Check.Equal(1, output.Published.Count);
                    Check.That(output.Published["..\\escape.txt"].SequenceEqual("zipelo-safe-test-payload\n"u8.ToArray()),
                        "Linux 역슬래시 파일명이 경로 구분자로 해석됨");
                }
            }
            else
            {
                Check.Equal(0, output.CommitCalls);
                Check.Equal(0, output.Published.Count);
            }
        }
    }

    private static EngineStub Stub(Func<EngineContext, Task> execute) =>
        new((inputs, context, token) => execute(context));

    private static async Task Write(
        EngineContext context,
        string name,
        byte[] content,
        EntryKind kind = EntryKind.File,
        VerificationState verification = VerificationState.Passed)
    {
        using var decoded = new MemoryStream(content, false);
        await context.WriteEntryAsync(new(Guid.NewGuid().ToString("N"), name, kind, content.Length),
            decoded, _ => ValueTask.FromResult(verification));
    }

    private sealed record Run(JobResult Result, RecordingOutput Output, List<JobEvent> Events);

    private static async Task<Run> Execute(
        IArchiveEngine engine,
        RecordingOutput? output = null,
        JobLimits? limits = null,
        CancellationToken token = default,
        PasswordProvider? passwords = null,
        ArchiveAction action = ArchiveAction.Extract,
        Action<JobEvent>? observe = null)
    {
        using var input = new MemoryStream(new byte[] { 1, 2, 3 }, false);
        var events = new List<JobEvent>();
        output ??= new RecordingOutput();
        var result = await new ArchiveJob(Guid.NewGuid(), limits ?? Limits).ExecuteAsync(
            action, "zip", engine, [input], output, passwords ?? TestSecrets.None,
            new InlineProgress(value =>
            {
                events.Add(value);
                observe?.Invoke(value);
            }), token);
        Check.That(output.Disposed, "최종 결과 전에 출력 정리가 수행되지 않음");
        Check.Equal(result.State, events[^1].State);
        Check.That(events.Select(value => value.Sequence).SequenceEqual(
            Enumerable.Range(1, events.Count).Select(value => (long)value)), "이벤트 순서가 잘못됨");
        Check.That(events.All(value => value.JobId == result.JobId), "이벤트 작업 ID가 잘못됨");
        return new(result, output, events);
    }

    private static void Failed(Run run, ArchiveError error)
    {
        Check.Equal(JobState.Failed, run.Result.State);
        Check.Equal(error, run.Result.Error);
        Check.Equal(0, run.Output.CommitCalls);
        Check.Equal(0, run.Output.Published.Count);
    }

    private static void Cancelled(Run run)
    {
        Check.Equal(JobState.Cancelled, run.Result.State);
        Check.Equal(ArchiveError.Cancelled, run.Result.Error);
        Check.Equal(0, run.Output.CommitCalls);
        Check.Equal(0, run.Output.Published.Count);
    }
}
