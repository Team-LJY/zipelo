using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using Avalonia.Platform.Storage;
using Zipelo.Core;
using Zipelo.Ui;

namespace Zipelo.Ubuntu;

public partial class MainWindow : Window
{
    private readonly IUbuntuArchiveWorkspace workspace;
    private readonly Button openButton;
    private readonly Button stopButton;
    private readonly TextBlock archiveTitle;
    private readonly TextBlock availability;
    private readonly TextBlock status;
    private readonly TextBlock emptyState;
    private readonly ProgressBar progress;
    private readonly ListBox files;
    private readonly Button createButton;
    private readonly Button retryCreateButton;
    private readonly TextBox creationName;
    private readonly TextBlock creationSelection;
    private IReadOnlyList<IStorageFile> creationFiles = Array.Empty<IStorageFile>();
    private string? creationFolder;
    private readonly Button extractButton;
    private readonly TextBox extractionName;
    private readonly TextBlock extractionSelection;
    private SelectedArchive? openedArchive;
    private bool startupHandled;
    private bool busy;
    private bool stopping;
    private CancellationTokenSource? activeJob;
    private Guid? activeJobId;

    public MainWindow() : this(new UbuntuWorkspace()) { }

    public MainWindow(IUbuntuArchiveWorkspace workspace)
    {
        this.workspace = workspace;
        AvaloniaXamlLoader.Load(this);
        openButton = this.FindControl<Button>("OpenButton")!;
        stopButton = this.FindControl<Button>("StopButton")!;
        archiveTitle = this.FindControl<TextBlock>("ArchiveTitle")!;
        availability = this.FindControl<TextBlock>("Availability")!;
        status = this.FindControl<TextBlock>("Status")!;
        emptyState = this.FindControl<TextBlock>("EmptyState")!;
        progress = this.FindControl<ProgressBar>("Progress")!;
        files = this.FindControl<ListBox>("Files")!;
        createButton = this.FindControl<Button>("CreateButton")!;
        retryCreateButton = this.FindControl<Button>("RetryCreateButton")!;
        creationName = this.FindControl<TextBox>("CreationName")!;
        creationSelection = this.FindControl<TextBlock>("CreationSelection")!;
        extractButton = this.FindControl<Button>("ExtractButton")!;
        extractionName = this.FindControl<TextBox>("ExtractionName")!;
        extractionSelection = this.FindControl<TextBlock>("ExtractionSelection")!;
        Closed += (_, _) =>
        {
            foreach (var file in creationFiles) { file.Dispose(); }
            openedArchive?.Owner?.Dispose();
        };
        UpdateActions();
    }

    private sealed record SelectedArchive(string Name, Func<Task<Stream>> OpenRead, IDisposable? Owner = null);

    public async Task OpenStartupAsync(IReadOnlyList<string> arguments)
    {
        if (startupHandled) { return; }
        startupHandled = true;
        var target = ArchiveOpenArguments.Parse(arguments);
        if (target.Error is not null) { status.Text = target.Error; return; }
        if (target.Path is null) { return; }
        await OpenArchiveCoreAsync(() => Task.FromResult<SelectedArchive?>(new(
            Path.GetFileName(target.Path), () => Task.FromResult<Stream>(new FileStream(
                target.Path, FileMode.Open, FileAccess.Read, FileShare.Read, 65536,
                FileOptions.Asynchronous | FileOptions.SequentialScan)))));
    }

    private async void OpenArchive(object? sender, RoutedEventArgs e)
    {
        await OpenArchiveCoreAsync(async () =>
        {
            if (!StorageProvider.CanOpen)
            {
                status.Text = "파일 선택을 사용할 수 없습니다.";
                return null;
            }
            var selection = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
            {
                Title = "ZIP 압축파일 열기",
                AllowMultiple = false,
                FileTypeFilter = [new FilePickerFileType("ZIP 압축파일") { Patterns = ["*.zip"] }]
            });
            if (selection.Count == 0) { return null; }
            if (selection.Count != 1)
            {
                foreach (var file in selection) { file.Dispose(); }
                status.Text = "압축파일은 한 번에 하나만 열 수 있습니다.";
                return null;
            }
            var selected = selection[0];
            return new SelectedArchive(selected.Name, selected.OpenReadAsync, selected);
        });
    }

    private async Task OpenArchiveCoreAsync(Func<Task<SelectedArchive?>> select)
    {
        if (busy)
        {
            status.Text = "다른 작업이 진행 중입니다. 작업 후 파일 열기를 다시 요청하세요.";
            return;
        }
        if (!workspace.CanListZip)
        {
            status.Text = "작업 프로세스를 사용할 수 없습니다. 프로그램 폴더를 확인하세요.";
            UpdateActions();
            return;
        }
        busy = true;
        UpdateActions();
        SelectedArchive? selected = null;
        try
        {
            selected = await select();
            if (selected is null) { return; }
            var error = ArchiveOpenArguments.ValidateName(selected.Name);
            if (error is not null) { status.Text = error; return; }

            using var cancellation = new CancellationTokenSource();
            activeJob = cancellation;
            activeJobId = Guid.NewGuid();
            stopping = false;
            progress.IsVisible = true;
            status.Text = "파일 목록을 읽고 있습니다.";
            UpdateActions();

            // 파일 선택 제공자의 스트림을 사용하므로 포털 URI를 로컬 경로로 추측하지 않음.
            await using var input = await selected.OpenRead();
            var jobId = activeJobId.Value;
            var result = await workspace.ListAsync(jobId, input, cancellation.Token);
            if (activeJobId != result.JobId)
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            if (result.State == JobState.Completed && result.Error == ArchiveError.None)
            {
                // 이전 목록은 성공적으로 읽은 결과가 있을 때만 교체함.
                files.ItemsSource = result.Entries.Select(entry => new ArchiveRow(
                    entry.Entry.Name,
                    entry.Entry.DeclaredSize is long size ? $"{size:N0}바이트" : "알 수 없음",
                    entry.Verification switch
                    {
                        VerificationState.Passed => "검증 통과",
                        VerificationState.Failed => "검증 실패",
                        VerificationState.Unavailable => "검증 불가",
                        _ => "검증 전"
                    })).ToArray();
                var previousArchive = openedArchive;
                openedArchive = selected;
                previousArchive?.Owner?.Dispose();
                archiveTitle.Text = selected.Name;
                emptyState.Text = "이 압축파일에는 표시할 항목이 없습니다.";
                emptyState.IsVisible = result.Entries.Count == 0;
                status.Text = $"항목 {result.Entries.Count:N0}개를 읽었습니다. 내용 검증은 별도입니다.";
            }
            else if (result.State == JobState.Cancelled)
            {
                status.Text = "목록 읽기가 중지되었습니다.";
            }
            else
            {
                status.Text = ErrorMessage(result.Error);
            }
        }
        catch (OperationCanceledException)
        {
            status.Text = "목록 읽기 요청이 취소되었습니다.";
        }
        catch (ArchiveException error)
        {
            status.Text = ErrorMessage(error.Code);
        }
        catch (Exception)
        {
            // 포털과 작업자 예외 원문에 포함될 수 있는 파일 경로를 노출하지 않음.
            status.Text = "파일을 읽지 못했습니다. 파일과 프로그램 폴더를 확인하고 다시 시도하세요.";
        }
        finally
        {
            try
            {
                if (!ReferenceEquals(selected, openedArchive)) { selected?.Owner?.Dispose(); }
            }
            catch (Exception) { status.Text = "파일 핸들 정리를 완료하지 못했습니다. 다시 열기 전에 파일 상태를 확인하세요."; }
            activeJob = null;
            activeJobId = null;
            progress.IsVisible = false;
            busy = false;
            stopping = false;
            UpdateActions();
        }
    }

    private async void CreateArchive(object? sender, RoutedEventArgs e) => await RunCreation(true);

    private async void RetryCreation(object? sender, RoutedEventArgs e) => await RunCreation(false);

    private async Task RunCreation(bool choose)
    {
        if (busy || !workspace.CanCreateZip) { return; }
        busy = true;
        UpdateActions();
        try
        {
            if (choose)
            {
                if (!StorageProvider.CanOpen || !StorageProvider.CanPickFolder)
                {
                    status.Text = "파일과 저장 폴더를 선택할 수 없습니다.";
                    return;
                }
                var selection = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
                {
                    Title = "ZIP에 넣을 파일 선택 (폴더 제외)", AllowMultiple = true
                });
                if (selection.Count == 0) { return; }
                foreach (var file in creationFiles) { file.Dispose(); }
                creationFiles = selection;
                creationFolder = null;
                creationSelection.Text = $"선택한 파일 {selection.Count:N0}개: {string.Join(", ", selection.Select(file => file.Name))}";
                var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
                {
                    Title = "새 ZIP을 저장할 폴더 선택", AllowMultiple = false
                });
                if (folders.Count == 0) { return; }
                using var folder = folders[0];
                // 네이티브 게시 API에는 명시적으로 선택된 로컬 폴더만 전달함.
                if (!folder.Path.IsFile)
                {
                    creationFolder = null;
                    status.Text = "현재는 로컬 저장 폴더만 지원합니다.";
                    return;
                }
                creationFolder = folder.Path.LocalPath;
            }
            if (creationFiles.Count == 0 || creationFolder is null) { return; }
            var name = creationName.Text ?? "";
            if (OutputPathPolicy.Validate(name, OutputPathPolicy.CurrentPolicy).Count != 1 ||
                !name.EndsWith(".zip", StringComparison.OrdinalIgnoreCase))
            {
                status.Text = "경로 없이 .zip으로 끝나는 새 파일 이름을 입력하세요.";
                return;
            }
            var destination = Path.Combine(creationFolder, name);
            creationSelection.Text = $"선택한 파일 {creationFiles.Count:N0}개 → {destination}";
            using var cancellation = new CancellationTokenSource();
            activeJob = cancellation;
            activeJobId = Guid.NewGuid();
            stopping = false;
            progress.IsVisible = true;
            status.Text = "선택한 파일을 읽고 새 ZIP을 생성하고 있습니다.";
            UpdateActions();
            var sources = creationFiles.Select(file => new CreationSource(file.Name, async token =>
            {
                token.ThrowIfCancellationRequested();
                return await file.OpenReadAsync();
            })).ToArray();
            var result = await workspace.CreateAsync(activeJobId.Value, sources, destination, cancellation.Token);
            if (result.Job.JobId != activeJobId) { throw new ArchiveException(ArchiveError.EngineFailure); }
            // 게시 후 늦은 취소로 실제 저장 성공을 취소 표시하지 않음.
            status.Text = ZipCreationWorkflow.Describe(result);
        }
        catch (OperationCanceledException)
        {
            status.Text = "생성 요청이 취소되었습니다. 선택한 파일과 저장 위치는 유지됩니다.";
        }
        catch (ArchiveException error) { status.Text = ErrorMessage(error.Code); }
        catch (Exception)
        {
            status.Text = "생성하지 못했습니다. 파일과 저장 위치를 확인하세요. 기존 목록과 선택은 유지됩니다.";
        }
        finally
        {
            activeJob = null;
            activeJobId = null;
            progress.IsVisible = false;
            busy = false;
            stopping = false;
            UpdateActions();
        }
    }

    private async void ExtractArchive(object? sender, RoutedEventArgs e)
    {
        var source = openedArchive;
        if (busy || source is null || !workspace.CanExtractZip) { return; }
        busy = true;
        UpdateActions();
        try
        {
            var name = extractionName.Text ?? "";
            ZipExtractionWorkflow.ValidateFolderName(name);
            if (!StorageProvider.CanPickFolder)
            {
                status.Text = "저장 위치 선택을 사용할 수 없습니다.";
                return;
            }
            var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
            {
                Title = "새 결과 폴더를 만들 부모 위치 선택", AllowMultiple = false
            });
            if (folders.Count == 0) { return; }
            using var parent = folders[0];
            if (!parent.Path.IsFile)
            {
                status.Text = "안전한 네이티브 게시에는 사용자가 고른 로컬 부모 폴더가 필요합니다.";
                return;
            }
            var destination = Path.Combine(parent.Path.LocalPath, name);
            extractionSelection.Text = $"새 결과 폴더: {destination}";
            using var cancellation = new CancellationTokenSource();
            var jobId = Guid.NewGuid();
            activeJobId = jobId;
            activeJob = cancellation;
            stopping = false;
            progress.IsVisible = true;
            status.Text = "ZIP 무결성과 새 결과 폴더 게시를 확인하고 있습니다.";
            UpdateActions();
            var result = await Task.Run(() => workspace.ExtractAsync(jobId, async token =>
            {
                token.ThrowIfCancellationRequested();
                return await source.OpenRead();
            }, destination, cancellation.Token));
            if (activeJobId != jobId || result.Job.JobId != jobId)
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            status.Text = ZipExtractionWorkflow.Describe(result);
        }
        catch (OperationCanceledException)
        {
            status.Text = "압축 풀기가 취소되었습니다. 기존 목록과 선택은 유지됩니다.";
        }
        catch (ArchiveException error) { status.Text = ErrorMessage(error.Code); }
        catch (Exception)
        {
            status.Text = "압축 풀기를 완료하지 못했습니다. 파일과 새 저장 위치를 확인하세요. 목록과 선택은 유지됩니다.";
        }
        finally
        {
            activeJob = null;
            activeJobId = null;
            progress.IsVisible = false;
            busy = false;
            stopping = false;
            UpdateActions();
        }
    }

    private void StopJob(object? sender, RoutedEventArgs e) => RequestStop();

    private void RequestStop()
    {
        if (activeJob is null || stopping) { return; }
        stopping = true;
        try { activeJob.Cancel(); }
        catch (AggregateException) { }
        status.Text = "작업 프로세스의 종료를 기다리고 있습니다.";
        UpdateActions();
    }

    private void UpdateActions()
    {
        openButton.IsEnabled = !busy && workspace.CanListZip;
        createButton.IsEnabled = !busy && workspace.CanCreateZip;
        createButton.Content = ZipCreationWorkflow.ValidationEnabled ? "ZIP 생성 검증" : "새로 압축하기";
        retryCreateButton.IsEnabled = !busy && workspace.CanCreateZip && creationFiles.Count > 0 && creationFolder is not null;
        creationName.IsEnabled = !busy && workspace.CanCreateZip;
        extractButton.IsEnabled = !busy && openedArchive is not null && workspace.CanExtractZip;
        extractButton.Content = ZipExtractionWorkflow.ValidationEnabled ? "압축 풀기 검증" : "압축 풀기";
        extractButton.SetValue(ToolTip.TipProperty, ZipExtractionWorkflow.ScopeMessage);
        extractionName.IsEnabled = !busy && workspace.CanExtractZip;
        stopButton.IsEnabled = activeJob is not null && !stopping;
        availability.Text = workspace.AvailabilityMessage;
    }

    private void OnClosing(object? sender, WindowClosingEventArgs e)
    {
        if (!busy) { return; }
        e.Cancel = true;
        RequestStop();
        status.Text = "파일 선택이나 작업 정리가 끝난 뒤 창을 닫아주세요.";
    }

    private static string ErrorMessage(ArchiveError error) => error switch
    {
        ArchiveError.OutputConflict => "같은 이름의 파일이 있습니다. 기존 파일은 유지됩니다. 새 이름을 입력하세요.",
        ArchiveError.UnsafePath => "파일 이름을 확인하세요. 중복 이름이나 경로가 포함된 이름은 사용할 수 없습니다.",
        ArchiveError.EngineUnavailable => "작업 프로세스를 시작할 수 없습니다. 프로그램 폴더를 확인하세요.",
        ArchiveError.UnsupportedOperation => "이 ZIP 형식이나 암호 방식은 현재 연결에서 지원하지 않습니다.",
        ArchiveError.BudgetExceeded => "처리 크기나 시간 한도를 초과했습니다. 더 작은 파일로 시도하세요.",
        ArchiveError.IntegrityFailure => "데이터가 검증을 통과하지 못했습니다. 정상 파일로 간주하지 마세요.",
        _ => "작업 프로세스가 정상 결과를 반환하지 못했습니다. 다른 파일을 선택해 다시 시도하세요."
    };
}

public sealed record ArchiveRow(string Name, string Size, string Verification);
