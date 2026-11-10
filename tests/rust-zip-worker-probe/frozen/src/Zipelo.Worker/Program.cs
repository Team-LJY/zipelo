using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Worker;

// 요청 한 건을 처리하고 종료함. 경로나 암호를 명령행으로 받지 않음.
if (args.Length == 1 && args[0] == Zipelo.Runtime.Streaming.ExtractionWire.Mode)
{
    return await StreamingZipExtractor.RunAsync();
}
if (args.Length != 0) { return 2; }
AppContext.SetSwitch("System.IO.Compression.UseStrictValidation", true);
using var lifetime = new CancellationTokenSource(TimeSpan.FromMinutes(5));
var id = Guid.Empty;
ZipWorkerResult response;
try
{
    var request = await WorkerProtocol.ReadAsync<WorkerRequest>(Console.OpenStandardInput(), lifetime.Token);
    id = request.JobId;
    WorkerProtocol.ValidateLimits(request.Limits);
    if (id == Guid.Empty || request.Inputs is null || request.Names is null ||
        request.Inputs.Length == 0 || request.Inputs.Length > WorkerProtocol.MaxEntries ||
        request.Inputs.Any(input => input is null) ||
        request.Inputs.Sum(input => (long)input.Length) > WorkerProtocol.MaxDataBytes ||
        request.Names.Any(name => name is null || name.Length > WorkerProtocol.MaxNameLength) ||
        (request.Action is ArchiveAction.List or ArchiveAction.Extract && (request.Inputs.Length != 1 || request.Names.Length != 0)) ||
        (request.Action == ArchiveAction.Create && request.Names.Length != request.Inputs.Length))
    {
        throw new ArchiveException(ArchiveError.UnsupportedOperation);
    }
    if (request.Action == ArchiveAction.Extract)
    {
        response = ZipExtractor.Extract(request, lifetime.Token);
    }
    else
    {
        var inputs = request.Inputs.Select(input => (Stream)new MemoryStream(input, writable: false)).ToArray();
        try
        {
            var output = new BoundedMemoryOutput();
            var result = await new ArchiveJob(id, request.Limits).ExecuteAsync(request.Action, "zip",
                new BuiltInZipEngine(request.Names, request.Limits.MaxOutputBytes), inputs, output,
                (_, _) => throw new ArchiveException(ArchiveError.UnsupportedOperation), null, lifetime.Token);
            response = new(result, result.State == JobState.Completed ? output.CommittedBytes : null);
        }
        finally
        {
            foreach (var input in inputs) { input.Dispose(); }
        }
    }
}
catch (ArchiveException failure) { response = WorkerProtocol.Failure(id, failure.Code); }
catch (OperationCanceledException) { response = WorkerProtocol.Failure(id, ArchiveError.BudgetExceeded); }
catch { response = WorkerProtocol.Failure(id, ArchiveError.EngineFailure); }
try
{
    await WorkerProtocol.WriteAsync(Console.OpenStandardOutput(), response, lifetime.Token);
    return 0;
}
catch { return 1; }
