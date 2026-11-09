using Avalonia.Controls;
using Zipelo.Core;

namespace Zipelo.UbuntuUiHarness;

internal static class UiCases
{
    public static async Task RunAsync(string name, UiSession ui)
    {
        if (name is "wrong-job-preserve" or "old-job-preserve" or "picker-cancel-reopen" or "empty-result")
        {
            await PreserveAsync(name, ui);
            return;
        }
        var (task, call) = await ui.StartupAsync();
        if (name == "startup-success")
        {
            call.Complete();
            await UiSession.AwaitAsync(task);
            await ui.IdleAsync();
            ui.Success();
            await ui.Window.OpenStartupAsync([ui.Fixture.InputPath]);
            UiSession.Check(ui.Workspace.Calls.Count == 1, "startup must be one-shot");
            return;
        }
        if (name == "complete-then-stop")
        {
            call.Complete();
            await UiSession.AwaitAsync(task);
            await ui.IdleAsync();
            var status = ui.Control<TextBlock>("Status").Text;
            UiSession.Check(!ui.Click("StopButton"), "completed job accepts stop");
            UiSession.Check(call.CancelCount == 0 && !call.Token.IsCancellationRequested, "late cancel changed token");
            UiSession.Check(ui.Control<TextBlock>("Status").Text == status, "late cancel changed status");
            ui.Success();
            return;
        }
        if (name == "queued-complete-then-stop") { call.Complete(); }
        if (name == "close-waits") { ui.Window.Close(); }
        else { UiSession.Check(ui.Click("StopButton"), "initial stop unavailable"); }
        UiSession.Check(call.Token.IsCancellationRequested && call.CancelCount == 1, "cancel not observed once");
        UiSession.Check(ui.Control<ProgressBar>("Progress").IsVisible &&
            !ui.Control<Button>("OpenButton").IsEnabled && !ui.Control<Button>("StopButton").IsEnabled,
            "UI returned before Task completion");
        UiSession.Check(!ui.Click("StopButton"), "duplicate enabled stop");
        ui.Window.Close();
        UiSession.Check(ui.Window.IsVisible && call.CancelCount == 1, "close must defer without repeated cancel");
        UiSession.Check(!task.IsCompleted, "Task ended before controlled release");
        if (name is "stop-then-complete" or "queued-complete-then-stop")
        {
            if (name == "stop-then-complete") { call.Complete(); }
        }
        else if (name == "cancel-result") { call.Complete(JobState.Cancelled); }
        else { call.Cancel(); }
        await UiSession.AwaitAsync(task);
        await ui.IdleAsync();
        if (name is "stop-then-complete" or "queued-complete-then-stop") { ui.Success(); }
        else
        {
            var expected = name == "cancel-result" ? "목록 읽기가 중지되었습니다." : "목록 읽기 요청이 취소되었습니다.";
            UiSession.Check(ui.Control<TextBlock>("Status").Text == expected, "cancel final state");
            UiSession.Check(ui.Control<ListBox>("Files").ItemCount == 0, "cancel replaced rows");
        }
    }
    private static async Task PreserveAsync(string name, UiSession ui)
    {
        var previousCall = await ui.SeedAsync();
        var list = ui.Control<ListBox>("Files");
        var previousItems = list.ItemsSource;
        var previousSelection = list.SelectedItem;
        var previousTitle = ui.Control<TextBlock>("ArchiveTitle").Text;
        if (name == "picker-cancel-reopen")
        {
            var requests = ui.Picker.OpenCalls;
            ui.Picker.Enqueue(null);
            UiSession.Check(ui.Click("OpenButton"), "picker cancel entry");
            await ui.IdleAsync();
            UiSession.Check(ui.Picker.OpenCalls == requests + 1 && ui.Workspace.Calls.Count == 1,
                "cancel unexpectedly called workspace");
            Preserved();
        }
        var selection = new TestStorageFile("다시 열린 압축파일.zip");
        var call = await ui.ReopenAsync(selection);
        UiSession.Check(call.Id != previousCall.Id, "job ID reused");
        Preserved();
        if (name is "wrong-job-preserve" or "old-job-preserve")
        {
            call.Complete(resultId: name == "old-job-preserve" ? previousCall.Id : Guid.NewGuid());
            await ui.IdleAsync();
            Preserved();
            UiSession.Check(selection.Disposals == 1, "rejected selection owner not disposed");
            UiSession.Check(ui.Control<TextBlock>("Status").Text ==
                "작업 프로세스가 정상 결과를 반환하지 못했습니다. 다른 파일을 선택해 다시 시도하세요.", "wrong job accepted");
        }
        else
        {
            call.Complete(empty: name == "empty-result");
            await ui.IdleAsync();
            UiSession.Check(!ReferenceEquals(previousItems, list.ItemsSource), "reopen did not replace list");
            UiSession.Check(ui.Control<TextBlock>("ArchiveTitle").Text == selection.Name, "reopen title");
            UiSession.Check(selection.Disposals == 0, "opened owner disposed early");
            if (name == "empty-result")
            {
                UiSession.Check(list.ItemCount == 0 && ui.Control<TextBlock>("EmptyState").IsVisible, "empty result overlay");
            }
            else { ui.Success(); }
        }
        return;
        void Preserved()
        {
            UiSession.Check(ui.Control<TextBlock>("ArchiveTitle").Text == previousTitle, "previous title changed");
            UiSession.Check(ReferenceEquals(previousItems, list.ItemsSource) &&
                ReferenceEquals(previousSelection, list.SelectedItem), "previous list or selection changed");
        }
    }
}
