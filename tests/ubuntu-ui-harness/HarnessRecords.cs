using Avalonia;
using Avalonia.Controls;

namespace Zipelo.UbuntuUiHarness;

internal sealed record GeometryRecord(double X, double Y, double Width, double Height)
{
    public static GeometryRecord Capture(Rect value)
    {
        UiSession.Check(double.IsFinite(value.X) && double.IsFinite(value.Y) &&
            double.IsFinite(value.Width) && double.IsFinite(value.Height), "nonfinite geometry");
        return new GeometryRecord(value.X, value.Y, value.Width, value.Height);
    }
}

internal sealed record FailureDiagnostic(string Type, string Message, string? StackTrace,
    bool MessageTruncated, bool StackTraceTruncated, FailureDiagnostic? InnerException, bool InnerTruncated)
{
    public const int MessageLimit = 512;
    public const int StackLimit = 2048;
    public const int InnerLimit = 2;

    public static FailureDiagnostic Capture(Exception error, int depth = 0)
    {
        var message = error.Message;
        var stack = error.StackTrace;
        return new FailureDiagnostic(Limit(error.GetType().FullName ?? error.GetType().Name, 128)!,
            Limit(message, MessageLimit)!, Limit(stack, StackLimit), message.Length > MessageLimit,
            stack is not null && stack.Length > StackLimit, error.InnerException is not null && depth < InnerLimit
                ? Capture(error.InnerException, depth + 1) : null,
            error.InnerException is not null && depth >= InnerLimit);
    }
    private static string? Limit(string? value, int maximum) =>
        value is not null && value.Length > maximum ? value[..maximum] : value;
}

internal sealed record StartupObservation(int Invocations, int AcceptedCalls, object? Items, object? Selection,
    string? Title, string? Status, int RowCount, bool EmptyVisible, bool OpenEnabled, bool ProgressVisible, bool StopEnabled)
{
    public static StartupObservation Capture(UiSession ui)
    {
        var list = ui.Control<ListBox>("Files");
        return new StartupObservation(ui.Workspace.Invocations, ui.Workspace.Calls.Count,
            list.ItemsSource, list.SelectedItem, ui.Control<TextBlock>("ArchiveTitle").Text,
            ui.Control<TextBlock>("Status").Text, list.ItemCount, ui.Control<TextBlock>("EmptyState").IsVisible,
            ui.Control<Button>("OpenButton").IsEnabled, ui.Control<ProgressBar>("Progress").IsVisible,
            ui.Control<Button>("StopButton").IsEnabled);
    }
    public static void AssertReplay(StartupObservation before, StartupObservation after)
    {
        UiSession.Check(before.Invocations == 1 && after.Invocations == before.Invocations &&
            before.AcceptedCalls == 1 && after.AcceptedCalls == before.AcceptedCalls, "startup reentered workspace");
        UiSession.Check(ReferenceEquals(before.Items, after.Items) && ReferenceEquals(before.Selection, after.Selection) &&
            before.Title == after.Title && before.Status == after.Status && before.RowCount == after.RowCount &&
            before.EmptyVisible == after.EmptyVisible && before.OpenEnabled == after.OpenEnabled &&
            before.ProgressVisible == after.ProgressVisible && before.StopEnabled == after.StopEnabled,
            "startup replay changed successful UI state");
    }
}
