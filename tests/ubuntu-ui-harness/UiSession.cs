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
    public ControlledWorkspace Workspace { get; } = new();
    public TestStorageProvider Picker { get; }
    public Fixture Fixture { get; }
    public UiSession(TestStorageFactory factory, Fixture fixture)
    {
        Fixture = fixture;
        Picker = factory.Provider;
        Window = new MainWindow(Workspace);
        Check(ReferenceEquals(Window.StorageProvider, Picker) && factory.Created == 1, "test picker factory not bound");
        Window.Show();
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
    public async Task<ListCall> SeedAsync()
    {
        var (task, call) = await StartupAsync();
        call.Complete();
        await AwaitAsync(task);
        await IdleAsync();
        var list = Control<ListBox>("Files");
        Check(list.ItemCount == 2, "seed row count");
        list.SelectedIndex = 1;
        Check(list.SelectedItem is ArchiveRow, "seed selection");
        return call;
    }
    public async Task<ListCall> ReopenAsync(TestStorageFile selection)
    {
        Picker.Enqueue(selection);
        var count = Workspace.Calls.Count + 1;
        Check(Click("OpenButton"), "open button disabled");
        var call = await CallAsync(count);
        Active();
        return call;
    }
    public void Success()
    {
        Check(Control<TextBlock>("Status").Text == "항목 2개를 읽었습니다. 내용 검증은 별도입니다.", "completion status");
        Check(Control<ListBox>("Files").ItemCount == 2, "completion rows");
        Check(!Control<TextBlock>("EmptyState").IsVisible, "empty overlay on rows");
    }
    public async Task CleanupAsync()
    {
        await Workspace.DrainAsync();
        await IdleAsync();
        Window.Close();
        await WaitAsync(() => !Window.IsVisible, "window close");
        Check(Picker.Selections.All(file => file.Disposals == 1), "selection owners not disposed exactly once");
    }
}
