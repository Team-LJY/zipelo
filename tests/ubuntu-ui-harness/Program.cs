using System.Text.Json;
using Avalonia;
using Avalonia.Threading;
using Zipelo.Ubuntu;

namespace Zipelo.UbuntuUiHarness;

internal static class Program
{
    public static readonly string[] Cases =
    [
        "startup-success", "cancel-throws", "cancel-result", "close-waits", "complete-then-stop",
        "stop-then-complete", "queued-complete-then-stop", "startup-wrong-job", "startup-empty",
        "font-24", "font-32"
    ];

    [STAThread]
    public static int Main(string[] args)
    {
        var probe = args.Length == 2 && args[0] == "--probe" && args[1] is "before-show" or "after-show"
            ? args[1] : null;
        if (probe is null && (args.Length != 2 || args[0] != "--case" || !Cases.Contains(args[1]))) { return 2; }
        if (!OperatingSystem.IsLinux()) { return 2; }
        var selected = args[1];
        var exit = 124;
        try
        {
            var project = Fixture.FindProject();
            var guard = new SourceGuard(project);
            AppBuilder.Configure<Zipelo.Ubuntu.App>().UsePlatformDetect().SetupWithoutStarting();
            UiSession.Check(Application.Current is Zipelo.Ubuntu.App && Application.Current.ApplicationLifetime is null,
                "original App must have no desktop auto-start lifetime");
            using var loop = new CancellationTokenSource();
            Dispatcher.UIThread.Post(async () =>
            {
                MainWindow? ownedWindow = null;
                UiSession? ui = null;
                Fixture? fixture = null;
                var workspace = new ControlledWorkspace();
                var closed = false;
                var windowCreationStarted = false;
                var injectedFailure = false;
                try
                {
                    fixture = new Fixture(project);
                    windowCreationStarted = true;
                    ownedWindow = new MainWindow(workspace);
                    ownedWindow.Closed += (_, _) => closed = true;
                    ui = new UiSession(ownedWindow, workspace, fixture);
                    ui.Initialize(probe);
                    await UiSession.PumpAsync();
                    if (selected.StartsWith("font-", StringComparison.Ordinal)) { await LayoutCases.RunAsync(selected, ui); }
                    else { await UiCases.RunAsync(selected, ui); }
                    exit = 0;
                }
                catch (Exception error)
                {
                    injectedFailure = probe is not null && error is InvalidOperationException &&
                        error.Message == "injected initialization failure " +
                            (probe == "before-show" ? "before Show" : "after Show");
                    exit = 1;
                    Console.WriteLine(JsonSerializer.Serialize(new { kind = "failure", @case = selected,
                        type = error.GetType().Name, injectedFailure }));
                }
                finally
                {
                    var cleanupConfirmed = !windowCreationStarted || ownedWindow is not null;
                    async Task Confirm(Func<Task> cleanup)
                    {
                        try { await cleanup(); }
                        catch { cleanupConfirmed = false; }
                    }
                    await Confirm(async () =>
                    {
                        await workspace.DrainAsync();
                        if (ui is not null) { await ui.IdleAsync(); }
                    });
                    await Confirm(async () =>
                    {
                        if (ownedWindow is null) { return; }
                        ownedWindow.Close();
                        await UiSession.WaitAsync(() => closed && !ownedWindow.IsVisible, "owned window closed");
                    });
                    await Confirm(() => { fixture?.Dispose(); return Task.CompletedTask; });
                    await Confirm(() => { guard.Verify(); return Task.CompletedTask; });
                    if (!cleanupConfirmed) { exit = 125; }
                    else if (probe is not null) { exit = injectedFailure && ownedWindow is not null && closed ? 0 : 1; }
                    Console.WriteLine(JsonSerializer.Serialize(new { kind = "result", @case = selected, exit,
                        scope = "controlled-startup-ui-only", cleanupConfirmed, windowClosed = closed,
                        stopFollowingCases = exit is 124 or 125,
                        workspaceTasksReturned = workspace.Calls.All(call => call.Returned.Task.IsCompleted),
                        cancellationCounts = workspace.Calls.Select(call => call.CancelCount).ToArray() }));
                    loop.Cancel();
                }
            });
            try { Dispatcher.UIThread.MainLoop(loop.Token); }
            catch (OperationCanceledException) when (loop.IsCancellationRequested) { }
        }
        catch (Exception error)
        {
            exit = 125;
            Console.WriteLine(JsonSerializer.Serialize(new { kind = "setup-failure", type = error.GetType().Name,
                stopFollowingCases = true }));
        }
        return exit;
    }
}
