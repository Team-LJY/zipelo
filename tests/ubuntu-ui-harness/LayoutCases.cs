using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Presenters;
using Avalonia.VisualTree;

namespace Zipelo.UbuntuUiHarness;

internal static class LayoutCases
{
    public static async Task RunAsync(string name, UiSession ui)
    {
        UiSession.Check(ui.Window.Width == 960 && ui.Window.Height == 720 &&
            ui.Window.MinWidth == 640 && ui.Window.MinHeight == 500, "window contract differs");
        var (startup, call) = await ui.StartupAsync();
        await UiSession.PumpAsync();
        var title = ui.Window.GetVisualDescendants().OfType<TextBlock>().Single(block => block.Text == "Zipelo");
        UiSession.Check(title.FontSize == 32 && ui.Control<TextBlock>("ArchiveTitle").FontSize == 20,
            "explicit title sizes differ");
        UiSession.Check(ui.Control<Button>("OpenButton").FontSize == 16, "App button style differs");
        UiSession.Check(ui.Control<TextBlock>("Status").FontSize == 16, "App text style differs");
        var textBox = ui.Control<TextBox>("CreationName");
        UiSession.Check(textBox.FontSize == 16, "TextBox baseline differs");
        var form = textBox.Parent as Panel ?? throw new InvalidOperationException("form panel missing");
        var probe = new ComboBox { Name = "HarnessComboProbe", ItemsSource = new[] { "시험 전용 선택 항목" }, SelectedIndex = 0 };
        form.Children.Add(probe);
        try
        {
            await UiSession.PumpAsync();
            UiSession.Check(probe.FontSize == 16, "ComboBox baseline differs");
            Report("before", ui);
            var font = name == "font-24" ? 24d : 32d;
            var status = ui.Control<TextBlock>("Status");
            var message = string.Concat(Enumerable.Repeat("시험 전용 긴 한글 상태입니다. 모든 줄을 스크롤로 확인합니다. ", 24));
            status.Text = message;

            ui.Window.FontSize = font;
            for (var pass = 0; pass < 4; pass++)
            {
                foreach (var control in ui.Window.GetVisualDescendants().OfType<Control>())
                {
                    if (control is TextBlock block) { block.FontSize = font; }
                    if (control is TemplatedControl templated) { templated.FontSize = font; }
                }
                await UiSession.PumpAsync();
            }
            var text = ui.Window.GetVisualDescendants().OfType<TextBlock>().ToArray();
            UiSession.Check(text.Length > 0 && text.All(block => block.FontSize == font), "realized TextBlock override differs");
            UiSession.Check(title.FontSize == font && textBox.FontSize == font && probe.FontSize == font &&
                ui.Control<Button>("OpenButton").FontSize == font, "named font overrides differ");
            foreach (var size in new[] { new Size(960, 720), new Size(640, 500) })
            {
                ui.Window.Width = size.Width;
                ui.Window.Height = size.Height;
                await UiSession.PumpAsync();
                UiSession.Check(status.Text == message, "long status changed");
                foreach (var control in ui.Window.GetVisualDescendants().OfType<Control>())
                {
                    if (control is TextBlock block) { UiSession.Check(block.FontSize == font, "late TextBlock font differs"); }
                    if (control is TemplatedControl templated) { UiSession.Check(templated.FontSize == font, "late control font differs"); }
                }
                var viewport = status.Parent as ScrollViewer ?? throw new InvalidOperationException("status viewport missing");
                await UiSession.WaitAsync(() => viewport.Extent.Height > viewport.Viewport.Height && viewport.Viewport.Height > 0,
                    "long status overflow");
                viewport.Offset = new Vector(0, viewport.Extent.Height - viewport.Viewport.Height);
                await UiSession.PumpAsync();
                UiSession.Check(Math.Abs(viewport.Offset.Y - (viewport.Extent.Height - viewport.Viewport.Height)) <= 1,
                    "status did not reach maximum offset");
                UiSession.Check(ui.Control<ListBox>("Files").Bounds.Height >= 168, "minimum list space lost");
                var stop = ui.Control<Button>("StopButton");
                UiSession.Check(stop.IsEffectivelyVisible && stop.IsEnabled, "active Stop unavailable");
                FullyVisible(stop, ui.Window);
                var presenter = viewport.GetVisualDescendants().OfType<ScrollContentPresenter>().Single();
                FullyVisible(presenter, ui.Window);
                var statusBounds = InWindow(status, ui.Window);
                var end = new Rect(statusBounds.X, statusBounds.Bottom - 0.5, statusBounds.Width, 0.5);
                UiSession.Check(Contains(VisibleRegion(status, ui.Window), end), "status bottom remains clipped");
                Report("after-" + size.Width + "x" + size.Height, ui);
            }
            call.Complete();
            await UiSession.AwaitAsync(startup);
            await ui.IdleAsync();
            Report("after", ui);
        }
        finally { form.Children.Remove(probe); }
    }
    private static Rect InWindow(Visual visual, Window window)
    {
        var first = visual.TranslatePoint(new Point(0, 0), window)
            ?? throw new InvalidOperationException("detached visual");
        var last = visual.TranslatePoint(new Point(visual.Bounds.Width, visual.Bounds.Height), window)
            ?? throw new InvalidOperationException("detached visual");
        return new Rect(first, last);
    }
    private static bool Contains(Rect outer, Rect inner) => inner.Width > 0 && inner.Height > 0 &&
        inner.Left >= outer.Left - 1 && inner.Top >= outer.Top - 1 &&
        inner.Right <= outer.Right + 1 && inner.Bottom <= outer.Bottom + 1;

    private static Rect VisibleRegion(Visual visual, Window window)
    {
        var visible = new Rect(window.ClientSize);
        foreach (var ancestor in visual.GetVisualAncestors().Prepend(visual))
        {
            UiSession.Check(ancestor.IsVisible, "hidden ancestor");
            UiSession.Check(ancestor.RenderTransform is null, "unsupported transformed clip");
            if (ancestor.ClipToBounds) { visible = visible.Intersect(InWindow(ancestor, window)); }
            if (ancestor.Clip is not null)
            {
                UiSession.Check(ancestor.Clip is Avalonia.Media.RectangleGeometry, "unsupported nonrectangular clip");
                var origin = ancestor.TranslatePoint(ancestor.Clip.Bounds.Position, window)
                    ?? throw new InvalidOperationException("detached clipping ancestor");
                visible = visible.Intersect(new Rect(origin, ancestor.Clip.Bounds.Size));
            }
        }
        return visible;
    }
    private static void FullyVisible(Visual visual, Window window) =>
        UiSession.Check(Contains(VisibleRegion(visual, window), InWindow(visual, window)), "control outside visible viewport");

    private static void Report(string phase, UiSession ui)
    {
        var values = ui.Window.GetVisualDescendants().OfType<Control>()
            .Where(control => control is TextBlock or Button or TextBox or ComboBox).Take(129).ToArray();
        UiSession.Check(values.Length <= 128, "realized control bound exceeded");
        foreach (var control in values)
        {
            var font = control is TextBlock block ? block.FontSize : ((TemplatedControl)control).FontSize;
            Console.WriteLine(JsonSerializer.Serialize(new { kind = "font", phase, type = control.GetType().Name,
                name = control.Name ?? "unnamed", fontDip = font, bounds = InWindow(control, ui.Window),
                screenOrigin = control.PointToScreen(new Point(0, 0)), renderScaling = ui.Window.RenderScaling }));
        }
    }
}
