using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamProtocolTests;

internal static class ValidatorTests
{
    private static void Reject(WorkerStreamValidator validator, WorkerStreamFrame frame, WorkerStreamError error)
    {
        Check.Equal(error, Check.Throws<WorkerStreamException>(() => Wire.Accept(validator, frame)).Error);
        Check.Equal(WorkerStreamPhase.Faulted, validator.Phase);
        Check.Equal<WorkerStreamTerminalStatus?>(null, validator.TerminalStatus);
        Check.Equal(WorkerStreamError.Faulted,
            Check.Throws<WorkerStreamException>(() => Wire.Accept(validator, Wire.Begin)).Error);
    }

    public static Task Identifiers()
    {
        var wrongJob = Wire.Begin with { Header = Wire.Begin.Header with { JobId = Wire.Item } };
        Reject(new(Wire.Job), wrongJob, WorkerStreamError.IdMismatch);
        Reject(new(Wire.Job), Wire.Frame(WorkerStreamFrameKind.BeginJob, 0, item: Wire.Item),
            WorkerStreamError.IdMismatch);
        foreach (var kind in new[] { WorkerStreamFrameKind.Data, WorkerStreamFrameKind.EndItem })
        {
            var payload = kind == WorkerStreamFrameKind.Data ? new byte[] { 1 } : Wire.Length(0);
            Reject(Wire.Active(), Wire.Frame(kind, 2, payload, Wire.OtherItem), WorkerStreamError.IdMismatch);
        }
        var between = new WorkerStreamValidator(Wire.Job);
        Wire.Accept(between, Wire.Begin);
        Reject(between, Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(0)), WorkerStreamError.IdMismatch);
        Reject(Wire.Active(), Wire.Frame(WorkerStreamFrameKind.EndJob, 2, [2], Wire.Item), WorkerStreamError.IdMismatch);
        return Task.CompletedTask;
    }

    public static Task Sequences()
    {
        foreach (var sequence in new ulong[] { 0, 1, 3, ulong.MaxValue })
        {
            Reject(Wire.Active(), Wire.Frame(WorkerStreamFrameKind.Data, sequence, [7], Wire.Item),
                WorkerStreamError.SequenceMismatch);
        }
        return Task.CompletedTask;
    }

    public static Task Transitions()
    {
        Reject(new(Wire.Job), Wire.End(0), WorkerStreamError.UnexpectedFrame);
        Reject(new(Wire.Job), Wire.Frame(WorkerStreamFrameKind.BeginItem, 0, Wire.Length(0), Wire.Item),
            WorkerStreamError.UnexpectedFrame);
        var between = new WorkerStreamValidator(Wire.Job);
        Wire.Accept(between, Wire.Begin);
        Reject(between, Wire.Frame(WorkerStreamFrameKind.Data, 1, [1], Wire.Item),
            WorkerStreamError.UnexpectedFrame);
        Reject(Wire.Active(), Wire.Frame(WorkerStreamFrameKind.BeginItem, 2, Wire.Length(0), Wire.OtherItem),
            WorkerStreamError.UnexpectedFrame);
        Reject(Wire.Active(), Wire.Frame(WorkerStreamFrameKind.BeginJob, 2), WorkerStreamError.UnexpectedFrame);
        return Task.CompletedTask;
    }

    public static Task DuplicateItem()
    {
        var validator = Wire.Active(0);
        Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.EndItem, 2, Wire.Length(0), Wire.Item));
        Reject(validator, Wire.Frame(WorkerStreamFrameKind.BeginItem, 3, Wire.Length(0), Wire.Item),
            WorkerStreamError.IdMismatch);
        return Task.CompletedTask;
    }

    public static Task ItemLengths()
    {
        Reject(Wire.Active(1), Wire.Frame(WorkerStreamFrameKind.EndItem, 2, Wire.Length(0), Wire.Item),
            WorkerStreamError.InvalidLength);
        var validator = Wire.Active(1);
        Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.Data, 2, [1], Wire.Item));
        Reject(validator, Wire.Frame(WorkerStreamFrameKind.EndItem, 3, Wire.Length(ulong.MaxValue), Wire.Item),
            WorkerStreamError.InvalidLength);
        Reject(Wire.Active(0), Wire.Frame(WorkerStreamFrameKind.Data, 2, [1], Wire.Item),
            WorkerStreamError.BudgetExceeded);
        return Task.CompletedTask;
    }

    public static Task TerminalStatus()
    {
        Reject(Wire.Active(), Wire.End(2), WorkerStreamError.UnexpectedFrame);
        foreach (var status in new byte[] { 0, 4, 255 })
        {
            Reject(Wire.Active(), Wire.End(2, status), WorkerStreamError.UnexpectedFrame);
        }
        foreach (var status in new byte[] { 2, 3 })
        {
            var validator = Wire.Active(1);
            Wire.Accept(validator, Wire.End(2, status));
            Check.Equal(WorkerStreamPhase.Terminal, validator.Phase);
            validator.CompleteInput();
            Check.Equal(WorkerStreamPhase.InputCompleted, validator.Phase);
            Check.Equal<WorkerStreamTerminalStatus?>((WorkerStreamTerminalStatus)status, validator.TerminalStatus);
        }
        return Task.CompletedTask;
    }

    public static Task Limits()
    {
        var defaults = new WorkerStreamLimits();
        foreach (var value in new[]
        {
            defaults with { MaxDataBytes = -1 }, defaults with { MaxDataBytes = long.MaxValue },
            defaults with { MaxWireBytes = 128 }, defaults with { MaxWireBytes = long.MaxValue },
            defaults with { MaxItems = -1 }, defaults with { MaxItems = int.MaxValue },
            defaults with { MaxFrames = 1 }, defaults with { MaxFrames = int.MaxValue },
            defaults with { MaxDuration = TimeSpan.Zero }, defaults with { MaxDuration = TimeSpan.MaxValue }
        })
        {
            Check.Throws<ArgumentOutOfRangeException>(() => new WorkerStreamValidator(Wire.Job, value));
        }
        Check.Throws<ArgumentException>(() => new WorkerStreamValidator(Guid.Empty));
        var minimal = new WorkerStreamValidator(Wire.Job, defaults with
        {
            MaxDataBytes = 0, MaxItems = 0, MaxFrames = 2, MaxWireBytes = 129
        });
        Wire.Accept(minimal, Wire.Begin);
        Wire.Accept(minimal, Wire.End(1));
        minimal.CompleteInput();
        Check.Equal(129L, minimal.TotalWireBytes);
        return Task.CompletedTask;
    }

    public static Task HeaderValidationDoesNotSpend()
    {
        var validator = new WorkerStreamValidator(Wire.Job);
        validator.ValidateHeader(Wire.Begin.Header);
        validator.ValidateHeader(Wire.Begin.Header);
        Check.Equal(0, validator.FramesAccepted);
        Check.Equal(0L, validator.TotalWireBytes);
        Wire.Accept(validator, Wire.Begin);
        Check.Equal(1, validator.FramesAccepted);
        Check.Equal(64L, validator.TotalWireBytes);
        return Task.CompletedTask;
    }

    public static Task WireBudget()
    {
        foreach (var budget in new long[] { 129, 273 })
        {
            var validator = new WorkerStreamValidator(Wire.Job, new() { MaxWireBytes = budget });
            Wire.Accept(validator, Wire.Begin);
            if (budget == 273)
            {
                Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(0), Wire.Item));
                Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.EndItem, 2, Wire.Length(0), Wire.Item));
            }
            Wire.Accept(validator, Wire.End((ulong)validator.FramesAccepted));
            Check.Equal(budget, validator.TotalWireBytes);
        }
        var insufficient = new WorkerStreamValidator(Wire.Job, new() { MaxWireBytes = 272 });
        Wire.Accept(insufficient, Wire.Begin);
        Wire.Accept(insufficient, Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(0), Wire.Item));
        Wire.Accept(insufficient, Wire.Frame(WorkerStreamFrameKind.EndItem, 2, Wire.Length(0), Wire.Item));
        Reject(insufficient, Wire.End(3), WorkerStreamError.BudgetExceeded);
        return Task.CompletedTask;
    }

    public static Task DataBudget()
    {
        // 64 KiB 버퍼 하나를 재사용해 16 MiB 누적 경계만 검사함.
        var validator = new WorkerStreamValidator(Wire.Job);
        Wire.Accept(validator, Wire.Begin);
        var chunk = new byte[65536];
        foreach (var id in new[] { Wire.Item, Wire.OtherItem })
        {
            Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.BeginItem,
                (ulong)validator.FramesAccepted, Wire.Length(8 * 1024 * 1024), id));
            for (var index = 0; index < 128; index++)
            {
                Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.Data,
                    (ulong)validator.FramesAccepted, chunk, id));
            }
            Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.EndItem,
                (ulong)validator.FramesAccepted, Wire.Length(8 * 1024 * 1024), id));
        }
        Check.Equal(16L * 1024 * 1024, validator.TotalDataBytes);
        Wire.Accept(validator, Wire.End((ulong)validator.FramesAccepted));
        validator.CompleteInput();

        var limited = Wire.Active(1, new() { MaxDataBytes = 1 });
        Wire.Accept(limited, Wire.Frame(WorkerStreamFrameKind.Data, 2, [1], Wire.Item));
        Wire.Accept(limited, Wire.Frame(WorkerStreamFrameKind.EndItem, 3, Wire.Length(1), Wire.Item));
        Reject(limited, Wire.Frame(WorkerStreamFrameKind.BeginItem, 4, Wire.Length(1), Wire.OtherItem),
            WorkerStreamError.BudgetExceeded);
        return Task.CompletedTask;
    }

    public static Task FrameBudget()
    {
        var validator = Wire.Active(2, new() { MaxFrames = 5 });
        Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.Data, 2, [1], Wire.Item));
        Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.Data, 3, [2], Wire.Item));
        Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.EndItem, 4, Wire.Length(2), Wire.Item));
        Check.Equal(5, validator.FramesAccepted);
        Reject(validator, Wire.End(5), WorkerStreamError.BudgetExceeded);
        return Task.CompletedTask;
    }

    public static Task ItemBudget()
    {
        var validator = Wire.Active(0, new() { MaxItems = 1 });
        Wire.Accept(validator, Wire.Frame(WorkerStreamFrameKind.EndItem, 2, Wire.Length(0), Wire.Item));
        Check.Equal(1, validator.ItemsAccepted);
        Reject(validator, Wire.Frame(WorkerStreamFrameKind.BeginItem, 3, Wire.Length(0), Wire.OtherItem),
            WorkerStreamError.BudgetExceeded);
        return Task.CompletedTask;
    }

    public static Task ArithmeticLengths()
    {
        foreach (var length in new[] { int.MinValue, -1, 0, 65537, int.MaxValue })
        {
            var frame = Wire.Frame(WorkerStreamFrameKind.Data, 2, [1], Wire.Item);
            Reject(Wire.Active(), frame with { Header = frame.Header with { PayloadLength = length } },
                WorkerStreamError.InvalidLength);
        }
        var validator = Wire.Active();
        var mismatched = Wire.Frame(WorkerStreamFrameKind.Data, 2, [1], Wire.Item) with { Payload = new byte[2] };
        Reject(validator, mismatched, WorkerStreamError.InvalidLength);
        return Task.CompletedTask;
    }
}
