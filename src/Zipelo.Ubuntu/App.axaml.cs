using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;

namespace Zipelo.Ubuntu;

public partial class App : Application
{
    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var window = new MainWindow(new UbuntuWorkspace());
            window.Opened += async (_, _) => await window.OpenStartupAsync(desktop.Args ?? []);
            desktop.MainWindow = window;
        }
        base.OnFrameworkInitializationCompleted();
    }
}
