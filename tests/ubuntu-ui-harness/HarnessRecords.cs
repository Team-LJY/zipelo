using System.Text.Json;
using Avalonia;
using Avalonia.Media;
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

internal sealed record MatrixRecord(double? M11, double? M12, double? M13,
    double? M21, double? M22, double? M23, double? M31, double? M32, double? M33,
    bool AllFinite, bool ExactIdentity)
{
    public static MatrixRecord Capture(Matrix matrix)
    {
        var finite = double.IsFinite(matrix.M11) && double.IsFinite(matrix.M12) && double.IsFinite(matrix.M13) &&
            double.IsFinite(matrix.M21) && double.IsFinite(matrix.M22) && double.IsFinite(matrix.M23) &&
            double.IsFinite(matrix.M31) && double.IsFinite(matrix.M32) && double.IsFinite(matrix.M33);
        var identity = finite && matrix.M11 == 1 && matrix.M12 == 0 && matrix.M13 == 0 &&
            matrix.M21 == 0 && matrix.M22 == 1 && matrix.M23 == 0 &&
            matrix.M31 == 0 && matrix.M32 == 0 && matrix.M33 == 1;
        return new MatrixRecord(Finite(matrix.M11), Finite(matrix.M12), Finite(matrix.M13),
            Finite(matrix.M21), Finite(matrix.M22), Finite(matrix.M23),
            Finite(matrix.M31), Finite(matrix.M32), Finite(matrix.M33), finite, identity);
    }
    private static double? Finite(double value) => double.IsFinite(value) ? value : null;
}

internal static class TransformGuard
{
    public static void RequireIdentity(ITransform? transform, string visualType = "direct", string? visualName = null)
    {
        if (transform is null) { return; }
        var matrix = MatrixRecord.Capture(transform.Value);
        if (matrix.AllFinite && matrix.ExactIdentity) { return; }
        static string? Limit(string? value) => value is not null && value.Length > 128 ? value[..128] : value;
        Console.WriteLine(JsonSerializer.Serialize(new { kind = "transform-rejection",
            visualType = Limit(visualType), visualName = Limit(visualName),
            transformType = Limit(transform.GetType().FullName ?? transform.GetType().Name), matrix }));
        UiSession.Check(false, "unsupported transformed clip");
    }
}
