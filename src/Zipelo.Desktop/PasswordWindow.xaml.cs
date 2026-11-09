using System.Security;
using System.Windows;

namespace Zipelo.Desktop;

public partial class PasswordWindow : Window
{
    public SecureString? SubmittedPassword { get; private set; }

    public PasswordWindow()
    {
        InitializeComponent();
        Loaded += (_, _) => Secret.Focus();
        Closed += (_, _) => Secret.Clear();
    }

    public SecureString? TakePassword()
    {
        var password = SubmittedPassword;
        SubmittedPassword = null;
        return password;
    }

    public void DiscardPassword()
    {
        SubmittedPassword?.Dispose();
        SubmittedPassword = null;
        Secret.Clear();
    }

    private void AcceptPassword(object sender, RoutedEventArgs e)
    {
        SubmittedPassword = Secret.SecurePassword;
        SubmittedPassword.MakeReadOnly();
        Secret.Clear();
        DialogResult = true;
    }
}
