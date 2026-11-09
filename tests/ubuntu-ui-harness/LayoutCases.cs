using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.VisualTree;

namespace Zipelo.UbuntuUiHarness;

internal static class LayoutCases
{
    public static async Task RunAsync(string name, UiSession ui)
    {
        await ui.SeedAsync();
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
            ui.Window.Width = 640;
            ui.Window.Height = 500;
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
            UiSession.Check(status.Text == message, "long status changed");
            var viewport = status.Parent as ScrollViewer ?? throw new InvalidOperationException("status viewport missing");
            await UiSession.WaitAsync(() => viewport.Extent.Height > viewport.Viewport.Height && viewport.Viewport.Height > 0,
                "long status overflow");
            viewport.Offset = new Vector(0, viewport.Extent.Height - viewport.Viewport.Height);
            await UiSession.PumpAsync();
            UiSession.Check(viewport.Offset.Y > 0, "status did not scroll");
            UiSession.Check(ui.Control<ListBox>("Files").Bounds.Height >= 168, "minimum list space lost");
            UiSession.Check(ui.Control<Button>("StopButton").Bounds.Height > 0, "stop disappeared");
            Report("after", ui);
        }
        finally { form.Children.Remove(probe); }
    }
    private static void Report(string phase, UiSession ui)
    {
        var values = ui.Window.GetVisualDescendants().OfType<Control>()
            .Where(control => control is TextBlock or Button or TextBox or ComboBox).Take(129).ToArray();
        UiSession.Check(values.Length <= 128, "realized control bound exceeded");
        foreach (var control in values)
        {
            var font = control is TextBlock block ? block.FontSize : ((TemplatedControl)control).FontSize;
            Console.WriteLine(JsonSerializer.Serialize(new { kind = "font", phase, type = control.GetType().Name,
                name = control.Name ?? "unnamed", fontDip = font, widthDip = control.Bounds.Width, heightDip = control.Bounds.Height }));
        }
    }
}
