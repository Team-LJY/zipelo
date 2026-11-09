using System.ComponentModel;
using System.IO;
using System.Security;
using System.Windows;
using System.Windows.Media;
using Microsoft.Win32;
using Zipelo.Core;
using Zipelo.Ui;

namespace Zipelo.Desktop;

public partial class MainWindow : Window
{
    private enum ScreenPhase { Idle, Selecting, Running, WaitingForPassword, Cancelling, Finishing }

    private readonly IArchiveWorkspace workspace;
    private readonly HashSet<Guid> passwordRequests = [];
    private ScreenPhase phase;
    private CancellationTokenSource? activeJob;
    private Guid? activeJobId;
    private long lastSequence;
    private string? archivePath;
    private PasswordWindow? passwordWindow;
    private string[] creationSources = [];
    private string? creationDestination;
    private bool startupHandled;

    public MainWindow(IArchiveWorkspace workspace)
    {
        this.workspace = workspace;
        InitializeComponent();
        Availability.Text = workspace is IWorkspaceCapabilities features
            ? features.AvailabilityMessage
            : "작업 프로세스가 아직 연결되지 않았습니다. 파일을 처리할 수 없습니다.";
        UpdateActions();
    }

    private bool CanExecute(WorkspaceAction action, string format = "zip") =>
        workspace.IsAvailable && workspace is IWorkspaceCapabilities features && features.CanExecute(action, format);

    public async Task OpenStartupAsync(IReadOnlyList<string> arguments)
    {
        if (startupHandled) { return; }
        startupHandled = true;
        var target = ArchiveOpenArguments.Parse(arguments);
        if (target.Error is not null) { SetStatus(target.Error, true); return; }
        if (target.Path is null) { return; }
        if (phase != ScreenPhase.Idle)
        {
            SetStatus("다른 작업이 진행 중이라 시작 파일을 열지 못했습니다. 작업 후 파일 열기를 사용하세요.", true);
            return;
        }
        await SelectAndRun(WorkspaceAction.List, () => new WorkspaceRequest(
            Guid.NewGuid(), WorkspaceAction.List, [target.Path]));
    }

    private async void OpenArchive(object sender, RoutedEventArgs e)
    {
        await SelectAndRun(WorkspaceAction.List, () =>
        {
            var dialog = new OpenFileDialog
            {
                Title = "압축파일 열기",
                Filter = "ZIP 압축파일|*.zip",
                CheckFileExists = true
            };
            return dialog.ShowDialog(this) == true
                ? new WorkspaceRequest(Guid.NewGuid(), WorkspaceAction.List, [dialog.FileName])
                : null;
        });
    }

    private async void CreateArchive(object sender, RoutedEventArgs e)
    {
        await SelectAndRun(WorkspaceAction.Create, () =>
        {
            var sources = new OpenFileDialog
            {
                Title = "압축할 파일 선택",
                Multiselect = true,
                CheckFileExists = true
            };
            if (sources.ShowDialog(this) != true) { return null; }
            creationSources = sources.FileNames;
            creationDestination = null;
            CreationSelection.Text = $"선택한 생성 파일 {creationSources.Length:N0}개: {string.Join(", ", creationSources.Select(path => Path.GetFileName(path)))}";
            var destination = new SaveFileDialog
            {
                Title = "새 ZIP 파일 저장 위치",
                Filter = "ZIP 압축파일|*.zip",
                DefaultExt = ".zip",
                AddExtension = true,
                FileName = creationDestination ?? "새 압축파일.zip",
                OverwritePrompt = false
            };
            if (destination.ShowDialog(this) != true) { return null; }
            creationDestination = destination.FileName;
            CreationSelection.Text += $" → {Path.GetFileName(creationDestination)}";
            return new WorkspaceRequest(Guid.NewGuid(), WorkspaceAction.Create, creationSources, creationDestination);
        });
    }

    private async void RetryCreation(object sender, RoutedEventArgs e)
    {
        await SelectAndRun(WorkspaceAction.Create, () => creationSources.Length > 0 && creationDestination is not null
            ? new WorkspaceRequest(Guid.NewGuid(), WorkspaceAction.Create, creationSources, creationDestination)
            : null);
    }

    private async void ExtractArchive(object sender, RoutedEventArgs e)
    {
        await SelectAndRun(WorkspaceAction.Extract, () =>
        {
            if (archivePath is null) { SetStatus("먼저 ZIP 목록을 열어주세요.", true); return null; }
            var name = ExtractionName.Text;
            ZipExtractionWorkflow.ValidateFolderName(name);
            var parent = new OpenFolderDialog
            {
                Title = "새 결과 폴더를 만들 부모 위치 선택 (기존 폴더에 병합하지 않음)",
                Multiselect = false
            };
            if (parent.ShowDialog(this) != true) { return null; }
            var destination = Path.Combine(parent.FolderName, name);
            ExtractionSelection.Text = $"새 결과 폴더: {destination}";
            return new WorkspaceRequest(Guid.NewGuid(), WorkspaceAction.Extract, [archivePath], destination);
        });
    }

    private async Task SelectAndRun(WorkspaceAction action, Func<WorkspaceRequest?> select)
    {
        if (phase != ScreenPhase.Idle) { return; }
        if (!CanExecute(action))
        {
            SetStatus(action == WorkspaceAction.Extract
                ? "일반 압축 풀기는 비활성입니다. 명시적인 개발 검증 빌드와 작업자가 필요합니다."
                : "이 기능의 작업 프로세스가 연결되지 않았거나 지원하지 않습니다.", true);
            return;
        }

        // 파일 선택부터 결과 반영까지 같은 게이트를 유지함.
        phase = ScreenPhase.Selecting;
        UpdateActions();
        try
        {
            var request = select();
            if (request is null) { return; }
            if (request.Action == WorkspaceAction.List)
            {
                var target = request.Sources.Count == 1
                    ? ArchiveOpenArguments.ValidatePath(request.Sources[0])
                    : new ArchiveOpenTarget(null, "압축파일은 한 번에 하나만 열 수 있습니다.");
                if (target.Error is not null) { SetStatus(target.Error, true); return; }
            }
            await RunJob(request);
        }
        catch (ArchiveException error)
        {
            SetStatus(WorkspaceMessages.Error(error.Code), true);
            Progress.Value = 0;
        }
        catch (Exception)
        {
            // 파일 선택과 엔진 예외의 원문은 화면과 로그에 노출하지 않음.
            SetStatus("작업을 시작하지 못했습니다. 파일 상태를 확인하고 다시 선택하세요.", true);
            Progress.Value = 0;
        }
        finally
        {
            Progress.IsIndeterminate = false;
            activeJob = null;
            activeJobId = null;
            passwordRequests.Clear();
            phase = ScreenPhase.Idle;
            UpdateActions();
        }
    }

    private async Task RunJob(WorkspaceRequest request)
    {
        using var cancellation = new CancellationTokenSource();
        activeJob = cancellation;
        activeJobId = request.JobId;
        lastSequence = 0;
        passwordRequests.Clear();
        phase = ScreenPhase.Running;
        Progress.Value = 0;
        Progress.IsIndeterminate = true;
        ResultSummary.Text = "";
        UpdateActions();
        SetStatus(request.Action switch
        {
            WorkspaceAction.List => "파일 목록을 읽고 있습니다.",
            WorkspaceAction.Extract => "ZIP 무결성과 새 폴더 게시를 확인하고 있습니다.",
            _ => "파일을 압축하고 있습니다."
        });

        var progress = new Progress<WorkspaceProgress>(value => ApplyProgress(request.JobId, value));
        try
        {
            var result = await Task.Run(() => workspace.ExecuteAsync(
                request, progress, RequestPasswordAsync, cancellation.Token));
            phase = ScreenPhase.Finishing;
            UpdateActions();
            if (activeJobId != request.JobId || result.JobId != request.JobId)
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }

            // 늦은 취소 토큰 대신 호스트의 실제 종료 결과를 사용함.
            PublishResult(request, result);
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
        {
            phase = ScreenPhase.Finishing;
            Progress.Value = 0;
            SetStatus("중지 요청 후 작업이 종료되었습니다. 결과 파일 상태를 확인하세요.");
        }
        catch (ArchiveException error)
        {
            phase = ScreenPhase.Finishing;
            Progress.Value = 0;
            SetStatus(WorkspaceMessages.Error(error.Code), true);
        }
        catch (Exception)
        {
            phase = ScreenPhase.Finishing;
            Progress.Value = 0;
            SetStatus("파일을 처리하지 못했습니다. 파일 상태와 저장 위치를 확인하고 다시 시도하세요.", true);
        }
        finally
        {
            // 수명이 끝난 작업의 이벤트와 암호 요청을 결과 반영 이후 거부함.
            activeJobId = null;
            activeJob = null;
            Progress.IsIndeterminate = false;
        }
    }

    private void ApplyProgress(Guid jobId, WorkspaceProgress value)
    {
        if (activeJobId != jobId || value.JobId != jobId || value.Sequence <= lastSequence ||
            phase is ScreenPhase.Idle or ScreenPhase.Selecting or ScreenPhase.Finishing)
        {
            return;
        }
        lastSequence = value.Sequence;
        if (value.State is JobState.Completed or JobState.Partial or JobState.Cancelled or JobState.Failed)
        {
            phase = ScreenPhase.Finishing;
            Progress.IsIndeterminate = false;
            SetStatus("작업 결과와 정리를 확인하고 있습니다.");
            UpdateActions();
            return;
        }
        if (phase == ScreenPhase.Cancelling || activeJob?.IsCancellationRequested == true || value.State == JobState.Cancelling)
        {
            phase = ScreenPhase.Cancelling;
            SetStatus("중지 요청을 처리하고 있습니다.");
            Progress.IsIndeterminate = true;
        }
        else if (passwordWindow is not null || value.State == JobState.WaitingForPassword)
        {
            phase = ScreenPhase.WaitingForPassword;
            SetStatus("암호 입력을 기다리고 있습니다.");
            Progress.IsIndeterminate = false;
        }
        else
        {
            phase = ScreenPhase.Running;
            Progress.IsIndeterminate = value.TotalBytes is null or <= 0;
            if (value.TotalBytes > 0)
            {
                Progress.Value = Math.Clamp(100.0 * value.ProcessedBytes / value.TotalBytes.Value, 0, 100);
            }
            SetStatus($"처리한 크기: {Math.Max(0, value.ProcessedBytes):N0}바이트");
        }
        UpdateActions();
    }

    private void PublishResult(WorkspaceRequest request, WorkspaceResult result)
    {
        if (request.Action == WorkspaceAction.Extract)
        {
            var extractionPublished = result.Extraction is not null && ZipExtractionWorkflow.Published(result.Extraction);
            Progress.Value = extractionPublished ? 100 : 0;
            ResultSummary.Text = "열린 ZIP 목록과 이름/위치는 유지됩니다. 다른 새 위치는 압축 풀기 검증에서 선택하세요.";
            SetStatus(result.Extraction is not null ? ZipExtractionWorkflow.Describe(result.Extraction)
                : "새 결과 폴더의 게시를 확인하지 못했습니다.", !extractionPublished);
            return;
        }
        if (request.Action == WorkspaceAction.Create)
        {
            Progress.Value = result.Creation?.Publication?.Published == true ? 100 : 0;
            ResultSummary.Text = "생성에 사용한 선택은 유지됩니다. 다른 저장 이름은 새로 압축하기에서 선택하세요.";
            SetStatus(result.Creation is not null ? ZipCreationWorkflow.Describe(result.Creation)
                : "새 파일의 게시를 확인하지 못했습니다.", result.Creation?.Publication?.Published != true);
            return;
        }
        var succeeded = result.Outcome == WorkspaceOutcome.Completed && result.Error == ArchiveError.None;
        Progress.Value = succeeded ? 100 : 0;
        if (request.Action == WorkspaceAction.List && succeeded)
        {
            archivePath = request.Sources[0];
            ArchiveTitle.Text = Path.GetFileName(archivePath);
            Files.ItemsSource = result.Entries;
            EmptyState.Text = "이 압축파일에는 표시할 항목이 없습니다.";
            EmptyState.Visibility = result.Entries.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        }
        var published = result.Entries.Count(entry => entry.Outcome == EntryOutcome.Extracted);
        var failed = result.Entries.Count(entry => entry.Outcome == EntryOutcome.Failed || entry.Error != ArchiveError.None);
        ResultSummary.Text = $"결과 항목 {result.Entries.Count:N0}개, 게시 확인 {published:N0}개, 실패 {failed:N0}개";
        if (succeeded)
        {
            SetStatus(request.Action == WorkspaceAction.List ? "파일 목록을 읽었습니다. 내용 검증은 별도입니다." : "ZIP 생성 작업이 완료되었습니다.");
        }
        else if (result.Outcome == WorkspaceOutcome.Cancelled && result.Error is ArchiveError.None or ArchiveError.Cancelled)
        {
            SetStatus("작업이 중지되었습니다. 결과 파일 상태를 확인하세요.");
        }
        else if (result.Outcome == WorkspaceOutcome.Partial)
        {
            SetStatus("일부 항목만 처리했습니다. 결과와 검증 상태를 확인하세요.", true);
        }
        else
        {
            SetStatus(WorkspaceMessages.Error(result.Error), true);
        }
    }

    private Task<SecureString?> RequestPasswordAsync(PasswordChallenge challenge, CancellationToken cancellationToken)
    {
        return Dispatcher.InvokeAsync<SecureString?>(() =>
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (activeJobId != challenge.JobId || challenge.RequestId == Guid.Empty ||
                phase is ScreenPhase.Finishing or ScreenPhase.Cancelling || passwordWindow is not null ||
                !passwordRequests.Add(challenge.RequestId))
            {
                throw new ArchiveException(ArchiveError.InvalidPasswordResponse);
            }

            phase = ScreenPhase.WaitingForPassword;
            SetStatus(challenge.Attempt > 1 ? "암호를 다시 확인해 입력하세요." : "암호 입력을 기다리고 있습니다.");
            Progress.IsIndeterminate = false;
            var dialog = new PasswordWindow { Owner = this };
            passwordWindow = dialog;
            UpdateActions();
            using var registration = cancellationToken.Register(() =>
                Dispatcher.BeginInvoke(new Action(() =>
                {
                    if (dialog.IsVisible) { dialog.Close(); }
                })));
            try
            {
                cancellationToken.ThrowIfCancellationRequested();
                if (dialog.ShowDialog() == true && !cancellationToken.IsCancellationRequested && activeJobId == challenge.JobId)
                {
                    return dialog.TakePassword();
                }
                RequestStop();
                return null;
            }
            finally
            {
                dialog.DiscardPassword();
                passwordWindow = null;
                if (phase != ScreenPhase.Finishing)
                {
                    phase = activeJob?.IsCancellationRequested == true ? ScreenPhase.Cancelling : ScreenPhase.Running;
                    SetStatus(phase == ScreenPhase.Cancelling ? "중지 요청을 처리하고 있습니다." : "작업을 계속하고 있습니다.");
                    Progress.IsIndeterminate = true;
                }
                UpdateActions();
            }
        }).Task;
    }

    private void StopJob(object sender, RoutedEventArgs e) => RequestStop();

    private void RequestStop()
    {
        if (activeJob is null || phase == ScreenPhase.Finishing) { return; }
        phase = ScreenPhase.Cancelling;
        try { activeJob.Cancel(); }
        catch (AggregateException)
        {
            // 취소 관찰자 실패가 있어도 실제 작업 종료를 기다림.
        }
        SetStatus("중지 요청을 처리하고 있습니다.");
        UpdateActions();
    }

    private void UpdateActions()
    {
        var ready = phase == ScreenPhase.Idle;
        OpenButton.IsEnabled = ready && CanExecute(WorkspaceAction.List);
        CreateButton.IsEnabled = ready && CanExecute(WorkspaceAction.Create);
        CreateButton.Content = ZipCreationWorkflow.ValidationEnabled ? "ZIP 생성 검증" : "새로 압축하기";
        CreateButton.ToolTip = ZipCreationWorkflow.ScopeMessage;
        RetryCreateButton.IsEnabled = ready && CanExecute(WorkspaceAction.Create) &&
            creationSources.Length > 0 && creationDestination is not null;
        ExtractButton.IsEnabled = ready && archivePath is not null && CanExecute(WorkspaceAction.Extract);
        ExtractButton.Content = ZipExtractionWorkflow.ValidationEnabled ? "압축 풀기 검증" : "압축 풀기";
        ExtractButton.ToolTip = ZipExtractionWorkflow.ScopeMessage;
        ExtractionName.IsEnabled = ready && CanExecute(WorkspaceAction.Extract);
        StopButton.IsEnabled = activeJob is not null &&
            phase is ScreenPhase.Running or ScreenPhase.WaitingForPassword;
    }

    private void SetStatus(string message, bool error = false)
    {
        Status.Text = message;
        Status.Foreground = error ? new SolidColorBrush(Color.FromRgb(164, 38, 44)) : SystemColors.ControlTextBrush;
    }

    private void ShowInstallation(object sender, RoutedEventArgs e)
    {
        MessageBox.Show(this,
            "설치 파일은 준비 중입니다.\n\n예정 흐름: 설치 파일 더블클릭 → 설치 → 완료\n\n기본 앱은 설치 후 Windows 설정에서 직접 선택합니다. 현재 화면은 설정을 변경하지 않습니다.",
            "설치와 기본 앱 설정", MessageBoxButton.OK, MessageBoxImage.Information);
    }

    private void OnClosing(object? sender, CancelEventArgs e)
    {
        if (phase != ScreenPhase.Idle)
        {
            e.Cancel = true;
            RequestStop();
            SetStatus("파일 선택이나 작업 정리가 끝난 뒤 창을 닫아주세요.");
        }
    }
}
