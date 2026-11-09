using Avalonia.Controls;
using Zipelo.Core;

namespace Zipelo.UbuntuUiHarness;

internal static class UiCases
{
    public static async Task RunAsync(string name, UiSession ui)
    {
        var (task, call) = await ui.StartupAsync();
        if (name is "startup-wrong-job" or "startup-empty")
        {
            var items = ui.Control<ListBox>("Files").ItemsSource;
            var title = ui.Control<TextBlock>("ArchiveTitle").Text;
            call.Complete(resultId: name == "startup-wrong-job" ? Guid.NewGuid() : call.Id,
                empty: name == "startup-empty");
            await UiSession.AwaitAsync(task);
            await ui.IdleAsync();
            UiSession.Check(ui.Control<ListBox>("Files").ItemCount == 0, "startup unexpectedly added rows");
            UiSession.Check(ui.Control<TextBlock>("EmptyState").IsVisible, "empty startup overlay missing");
            if (name == "startup-wrong-job")
            {
                UiSession.Check(ReferenceEquals(items, ui.Control<ListBox>("Files").ItemsSource) &&
                    title == ui.Control<TextBlock>("ArchiveTitle").Text, "initial state replaced by wrong job");
                UiSession.Check(ui.Control<TextBlock>("Status").Text ==
                    "작업 프로세스가 정상 결과를 반환하지 못했습니다. 다른 파일을 선택해 다시 시도하세요.", "wrong job accepted");
            }
            else
            {
                UiSession.Check(ui.Control<TextBlock>("Status").Text ==
                    "항목 0개를 읽었습니다. 내용 검증은 별도입니다.", "empty completion status");
                UiSession.Check(ui.Control<TextBlock>("ArchiveTitle").Text == "startup.zip", "startup title missing");
            }
            return;
        }
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
}
