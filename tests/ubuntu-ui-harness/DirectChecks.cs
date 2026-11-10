using System.Text.Json;
using Avalonia;
using Avalonia.Media;
using Avalonia.Media.Transformation;

namespace Zipelo.UbuntuUiHarness;

internal static class DirectChecks
{
    public static async Task RunAsync()
    {
        var passed = 0;
        void Trial(string name, Action check)
        {
            check();
            passed++;
            Console.WriteLine(JsonSerializer.Serialize(new { kind = "direct", name, passed }));
        }
        Trial("geometry-empty", () => Geometry(new Rect(0, 0, 0, 0)));
        Trial("geometry-zero-width", () => Geometry(new Rect(4, 5, 0, 12)));
        Trial("geometry-zero-height", () => Geometry(new Rect(4, 5, 12, 0)));
        Trial("geometry-finite", () => Geometry(new Rect(-4, 5, 12, 24)));
        Trial("geometry-nan-rejected", () => Reject(() => GeometryRecord.Capture(new Rect(double.NaN, 0, 1, 1))));
        Trial("geometry-infinity-rejected", () => Reject(() => GeometryRecord.Capture(new Rect(0, double.PositiveInfinity, 1, 1))));
        Trial("diagnostic-bounds", () =>
        {
            Exception error = new Exception("leaf");
            for (var index = 0; index < 4; index++)
            {
                error = new DiagnosticException(new string('m', 800), new string('s', 4000), error);
            }
            var diagnostic = FailureDiagnostic.Capture(error);
            UiSession.Check(diagnostic.Message.Length == 512 && diagnostic.StackTrace?.Length == 2048 &&
                diagnostic.MessageTruncated && diagnostic.StackTraceTruncated, "diagnostic field bounds");
            UiSession.Check(diagnostic.InnerException?.InnerException is { InnerException: null, InnerTruncated: true },
                "diagnostic inner bound");
            using var json = JsonDocument.Parse(JsonSerializer.Serialize(diagnostic));
            UiSession.Check(json.RootElement.GetProperty("Message").GetString()?.Length == 512, "diagnostic JSON changed");
        });
        var initial = new StartupObservation(1, 1, new object(), null, "startup.zip", "success", 2,
            false, true, false, false);
        Trial("replay-preserved", () => StartupObservation.AssertReplay(initial, initial with { }));

        var workspace = new ControlledWorkspace();
        using var firstInput = new MemoryStream(Fixture.Bytes, writable: false);
        var first = workspace.ListAsync(Guid.NewGuid(), firstInput, CancellationToken.None);
        try
        {
            UiSession.Check(workspace.Invocations == 1 && workspace.Calls.Count == 1, "first controlled entry");
            workspace.Calls[0].Complete();
            await first.WaitAsync(TimeSpan.FromSeconds(5));
            var rejected = false;
            using var repeatedInput = new MemoryStream(Fixture.Bytes, writable: false);
            try { await workspace.ListAsync(Guid.NewGuid(), repeatedInput, CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(5)); }
            catch (InvalidOperationException error) when (error.Message == "workspace call bound exceeded") { rejected = true; }
            Trial("replay-entry-before-guard", () =>
            {
                UiSession.Check(rejected && workspace.Invocations == 2 && workspace.Calls.Count == 1,
                    "guard hid workspace invocation");
                Reject(() => StartupObservation.AssertReplay(initial,
                    initial with { Invocations = workspace.Invocations, AcceptedCalls = workspace.Calls.Count }));
            });
        }
        finally
        {
            await workspace.DrainAsync();
            await first.WaitAsync(TimeSpan.FromSeconds(5));
        }
        Trial("replay-state-change", () =>
        {
            Reject(() => StartupObservation.AssertReplay(initial, initial with { Status = "changed" }));
            Reject(() => StartupObservation.AssertReplay(initial, initial with { Title = "changed" }));
            Reject(() => StartupObservation.AssertReplay(initial, initial with { Items = new object() }));
        });
        Trial("transform-null", () => TransformGuard.RequireIdentity(null));
        Trial("transform-operations-identity", () => TransformGuard.RequireIdentity(TransformOperations.Identity));
        Trial("transform-matrix-identity", () => TransformGuard.RequireIdentity(new MatrixTransform(Matrix.Identity)));
        Trial("transform-translation-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(Matrix.CreateTranslation(1, 2)))));
        Trial("transform-scale-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(Matrix.CreateScale(2, 1)))));
        Trial("transform-rotation-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(Matrix.CreateRotation(0.25)))));
        Trial("transform-skew-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(Matrix.CreateSkew(0.25, 0)))));
        Trial("transform-nan-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(new Matrix(double.NaN, 0, 0, 1, 0, 0)))));
        Trial("transform-infinity-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(new Matrix(1, 0, 0, 1, double.PositiveInfinity, 0)))));
        Trial("transform-near-identity-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(Matrix.CreateScale(double.BitIncrement(1), 1)))));
        Trial("transform-perspective-rejected", () => Reject(() =>
            TransformGuard.RequireIdentity(new MatrixTransform(new Matrix(1, 0, 0.25, 0, 1, 0, 0, 0, 1)))));
        Trial("identity-outside-window-rejected", () =>
        {
            TransformGuard.RequireIdentity(TransformOperations.Identity);
            Reject(() => LayoutCases.AssertContained(new Rect(0, 0, 100, 100), new Rect(95, 20, 10, 10)));
        });
        Trial("identity-outside-ancestor-clip-rejected", () =>
        {
            TransformGuard.RequireIdentity(TransformOperations.Identity);
            var visible = new Rect(0, 0, 100, 100).Intersect(new Rect(10, 10, 20, 20));
            Reject(() => LayoutCases.AssertContained(visible, new Rect(25, 15, 10, 10)));
        });
        Trial("identity-inside-clipped-window", () =>
        {
            TransformGuard.RequireIdentity(TransformOperations.Identity);
            var visible = new Rect(0, 0, 100, 100).Intersect(new Rect(10, 10, 20, 20));
            LayoutCases.AssertContained(visible, new Rect(15, 15, 10, 10));
        });
        Trial("matrix-diagnostic-finite-json", () =>
        {
            var matrix = MatrixRecord.Capture(new Matrix(double.NaN, 0, 0, 1, double.PositiveInfinity, 0));
            using var document = JsonDocument.Parse(JsonSerializer.Serialize(matrix));
            var root = document.RootElement;
            UiSession.Check(!matrix.AllFinite && !matrix.ExactIdentity &&
                root.GetProperty("M11").ValueKind == JsonValueKind.Null &&
                root.GetProperty("M31").ValueKind == JsonValueKind.Null, "nonfinite matrix diagnostic differs");
            UiSession.Check(root.EnumerateObject().Where(property => property.Value.ValueKind == JsonValueKind.Number)
                .All(property => double.IsFinite(property.Value.GetDouble())), "nonfinite matrix escaped into JSON");
        });
        UiSession.Check(passed == 25, "direct case count differs");
        Console.WriteLine(JsonSerializer.Serialize(new { kind = "direct-summary", passed,
            scope = "records-and-oracle-only-no-product-mutation-no-ui" }));
    }

    private static void Geometry(Rect rectangle)
    {
        var record = GeometryRecord.Capture(rectangle);
        using var document = JsonDocument.Parse(JsonSerializer.Serialize(record));
        var properties = document.RootElement.EnumerateObject().ToArray();
        UiSession.Check(properties.Select(property => property.Name).SequenceEqual(new[] { "X", "Y", "Width", "Height" }),
            "geometry contains derived properties");
        UiSession.Check(properties.All(property => double.IsFinite(property.Value.GetDouble())), "nonfinite geometry JSON");
        UiSession.Check(document.RootElement.GetProperty("X").GetDouble() == rectangle.X &&
            document.RootElement.GetProperty("Y").GetDouble() == rectangle.Y &&
            document.RootElement.GetProperty("Width").GetDouble() == rectangle.Width &&
            document.RootElement.GetProperty("Height").GetDouble() == rectangle.Height, "geometry values changed");
    }
    private static void Reject(Action action)
    {
        var rejected = false;
        try { action(); }
        catch (InvalidOperationException) { rejected = true; }
        UiSession.Check(rejected, "invalid observation accepted");
    }
    private sealed class DiagnosticException(string message, string stack, Exception inner) : Exception(message, inner)
    {
        public override string StackTrace => stack;
    }
}
