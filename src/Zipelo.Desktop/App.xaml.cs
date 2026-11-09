using System.Windows;

namespace Zipelo.Desktop;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        var fallback = new UnavailableWorkspace();
        var workspace = ZipWorkerWorkspace.TryCreate(out var unavailableReason);
        fallback.AvailabilityMessage = unavailableReason;
        var window = new MainWindow(workspace ?? fallback);
        MainWindow = window;
        window.ContentRendered += async (_, _) => await window.OpenStartupAsync(e.Args);
        window.Show();
    }
}
