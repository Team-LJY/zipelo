using System.Text.Json;
using Avalonia;
using Avalonia.Controls.Platform;
using Avalonia.Threading;

namespace Zipelo.UbuntuUiHarness;

internal static class Program
{
    public static readonly string[] Cases =
    [
        "startup-success", "cancel-throws", "cancel-result", "close-waits", "complete-then-stop",
        "stop-then-complete", "queued-complete-then-stop", "wrong-job-preserve", "old-job-preserve",
        "picker-cancel-reopen", "empty-result", "font-24", "font-32"
    ];
    [STAThread]
    public static int Main(string[] args)
    {
        if (args.Length != 2 || args[0] != "--case" || !Cases.Contains(args[1])) { return 2; }
        if (!OperatingSystem.IsLinux()) { return 2; }
        var selected = args[1];
        var exit = 124;
        try
        {
            var project = Fixture.FindProject();
            var guard = new SourceGuard(project);
            var factory = new TestStorageFactory();
            AppBuilder.Configure<Zipelo.Ubuntu.App>().UsePlatformDetect()
                .With<IStorageProviderFactory>(factory).SetupWithoutStarting();
            UiSession.Check(Application.Current is Zipelo.Ubuntu.App && Application.Current.ApplicationLifetime is null,
                "original App must have no desktop auto-start lifetime");
            using var loop = new CancellationTokenSource();
            Dispatcher.UIThread.Post(async () =>
            {
                UiSession? ui = null;
                Fixture? fixture = null;
                try
                {
                    fixture = new Fixture(project);
                    ui = new UiSession(factory, fixture);
                    await UiSession.PumpAsync();
                    if (selected.StartsWith("font-", StringComparison.Ordinal)) { await LayoutCases.RunAsync(selected, ui); }
                    else { await UiCases.RunAsync(selected, ui); }
                    exit = 0;
                }
                catch (Exception error)
                {
                    exit = 1;
                    Console.WriteLine(JsonSerializer.Serialize(new { kind = "failure", @case = selected,
                        type = error.GetType().Name, reason = error is InvalidOperationException ? error.Message : "bounded phase failed" }));
                }
                finally
                {
                    try
                    {
                        if (ui is not null) { await ui.CleanupAsync(); }
                        fixture?.Dispose();
                        guard.Verify();
                        Console.WriteLine(JsonSerializer.Serialize(new { kind = "result", @case = selected, exit,
                            scope = "controlled-public-ui-only", sourceUnchanged = true, fixtureRemoved = true,
                            workspaceTasksReturned = ui?.Workspace.Calls.All(call => call.Returned.Task.IsCompleted) ?? true,
                            cancellationCounts = ui?.Workspace.Calls.Select(call => call.CancelCount).ToArray() }));
                    }
                    catch (Exception error)
                    {
                        exit = 125;
                        Console.WriteLine(JsonSerializer.Serialize(new { kind = "cleanup-unconfirmed", @case = selected,
                            type = error.GetType().Name, stopFollowingCases = true }));
                    }
                    loop.Cancel();
                }
            });
            try { Dispatcher.UIThread.MainLoop(loop.Token); }
            catch (OperationCanceledException) when (loop.IsCancellationRequested) { }
        }
        catch (Exception error)
        {
            exit = 1;
            Console.WriteLine(JsonSerializer.Serialize(new { kind = "setup-failure", type = error.GetType().Name }));
        }
        return exit;
    }
}
