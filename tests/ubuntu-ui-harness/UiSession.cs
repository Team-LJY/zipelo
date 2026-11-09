using System.Diagnostics;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Zipelo.Core;
using Zipelo.Ubuntu;

namespace Zipelo.UbuntuUiHarness;

internal sealed class UiSession
{
    public MainWindow Window { get; }
    public ControlledWorkspace Workspace { get; }
    public Fixture Fixture { get; }
    public UiSession(MainWindow window, ControlledWorkspace workspace, Fixture fixture)
    {
        Window = window;
        Workspace = workspace;
        Fixture = fixture;
    }
    public void Initialize(string? probe)
    {
        Check(probe != "before-show", "injected initialization failure before Show");
        Window.Show();
        Check(probe != "after-show", "injected initialization failure after Show");
    }
    public T Control<T>(string name) where T : Control => Window.FindControl<T>(name)
        ?? throw new InvalidOperationException("missing public control: " + name);
    public static void Check(bool condition, string label)
    {
        if (!condition) { throw new InvalidOperationException(label); }
    }
    public static async Task WaitAsync(Func<bool> predicate, string label)
    {
        var timer = Stopwatch.StartNew();
        while (true)
        {
            Check(timer.Elapsed < TimeSpan.FromSeconds(5), "phase deadline: " + label);
            if (predicate()) { return; }
            await Task.Delay(16);
        }
    }
    public static async Task AwaitAsync(Task task)
    {
        var timer = Stopwatch.StartNew();
        await task.WaitAsync(TimeSpan.FromSeconds(5));
        Check(timer.Elapsed < TimeSpan.FromSeconds(5), "late phase completion");
    }
    public static async Task PumpAsync()
    {
        await Dispatcher.UIThread.InvokeAsync(() => { }, DispatcherPriority.Background);
    }
    public bool Click(string name)
    {
        var button = Control<Button>(name);
        if (!button.IsEnabled || !button.IsVisible) { return false; }
        button.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        return true;
    }
    public void Active()
    {
        Check(Control<ProgressBar>("Progress").IsVisible, "progress not active");
        Check(!Control<Button>("OpenButton").IsEnabled, "open active during work");
        Check(Control<Button>("StopButton").IsEnabled, "stop not active");
    }
    public async Task IdleAsync()
    {
        await WaitAsync(() => Control<Button>("OpenButton").IsEnabled && !Control<ProgressBar>("Progress").IsVisible,
            "return to idle");
        Check(!Control<Button>("StopButton").IsEnabled, "stop active after completion");
    }
    public async Task<ListCall> CallAsync(int count)
    {
        await WaitAsync(() => Workspace.Calls.Count == count, "workspace entry");
        return Workspace.Calls[count - 1];
    }
    public async Task<(Task Task, ListCall Call)> StartupAsync()
    {
        var task = Window.OpenStartupAsync([Fixture.InputPath]);
        var call = await CallAsync(1);
        Active();
        return (task, call);
    }
    public void Success()
    {
        Check(Control<TextBlock>("Status").Text == "항목 2개를 읽었습니다. 내용 검증은 별도입니다.", "completion status");
        Check(Control<ListBox>("Files").ItemCount == 2, "completion rows");
        Check(!Control<TextBlock>("EmptyState").IsVisible, "empty overlay on rows");
    }
}
