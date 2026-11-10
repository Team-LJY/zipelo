using System.Diagnostics;

namespace Zipelo.StreamingExtractionTests;

internal sealed record RelayPlan(Guid Job, Invocation Invocation);
internal sealed record RelayProof(Guid Job, int RelayPid, int WorkerPid, Invocation Invocation,
    int ExitCode, bool ExitConfirmed);

internal static class ListRelay
{
    internal static async Task<int> Run()
    {
        var root = Environment.CurrentDirectory;
        var plan = Data.Read<RelayPlan>(Path.Combine(root, "list-relay.json"), 128 * 1024);
        Check.That(plan.Job != Guid.Empty && plan.Invocation.Arguments.Length <= 1, "목록 relay 계약 오류");
        Evidence.VerifyFiles(plan.Invocation);
        var start = new ProcessStartInfo
        {
            FileName = plan.Invocation.Executable,
            UseShellExecute = false,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true
        };
        foreach (var argument in plan.Invocation.Arguments) { start.ArgumentList.Add(argument); }
        using var process = new Process { StartInfo = start };
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        Check.That(process.Start(), "목록 작업자 시작 실패");
        var token = deadline.Token;
        var input = ForwardInput();
        var output = process.StandardOutput.BaseStream.CopyToAsync(Console.OpenStandardOutput(), token);
        var error = process.StandardError.BaseStream.CopyToAsync(Stream.Null, token);
        try
        {
            await Task.WhenAll(input, output, error, process.WaitForExitAsync(token));
            Evidence.VerifyFiles(plan.Invocation);
            var actual = new Invocation(plan.Invocation.Kind, process.StartInfo.FileName,
                plan.Invocation.ExecutableSha256, plan.Invocation.Worker, plan.Invocation.WorkerSha256,
                process.StartInfo.ArgumentList.ToArray());
            Data.WriteNew(Path.Combine(root, "list-identity.json"),
                new RelayProof(plan.Job, Environment.ProcessId, process.Id, actual, process.ExitCode, process.HasExited));
            return process.ExitCode;
        }
        finally
        {
            deadline.Cancel();
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
                await process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
            }
            try { await Task.WhenAll(input, output, error).WaitAsync(TimeSpan.FromSeconds(1)); }
            catch (OperationCanceledException) { }
        }

        async Task ForwardInput()
        {
            await Console.OpenStandardInput().CopyToAsync(process.StandardInput.BaseStream, token);
            process.StandardInput.Close();
        }
    }

    internal static bool Matches(RelayProof proof, Guid job, Invocation invocation) =>
        proof.Job == job && proof.RelayPid > 0 && proof.WorkerPid > 0 &&
        proof.RelayPid != proof.WorkerPid && proof.ExitConfirmed &&
        Evidence.Same(proof.Invocation, invocation);
}
