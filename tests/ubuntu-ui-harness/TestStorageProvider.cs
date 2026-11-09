using Avalonia.Controls;
using Avalonia.Controls.Platform;
using Avalonia.Platform.Storage;

namespace Zipelo.UbuntuUiHarness;

internal sealed class TestStorageFactory : IStorageProviderFactory
{
    public TestStorageProvider Provider { get; } = new();
    public int Created { get; private set; }
    public IStorageProvider CreateProvider(TopLevel topLevel)
    {
        Created++;
        return Provider;
    }
}

internal sealed class TestStorageProvider : IStorageProvider
{
    private readonly Queue<IReadOnlyList<IStorageFile>> choices = new();
    public int OpenCalls { get; private set; }
    public List<TestStorageFile> Selections { get; } = [];
    public bool CanOpen => true;
    public bool CanSave => false;
    public bool CanPickFolder => false;
    public void Enqueue(TestStorageFile? file)
    {
        UiSession.Check(choices.Count < 3, "picker queue bound exceeded");
        if (file is not null) { Selections.Add(file); }
        choices.Enqueue(file is null ? Array.Empty<IStorageFile>() : [file]);
    }
    public Task<IReadOnlyList<IStorageFile>> OpenFilePickerAsync(FilePickerOpenOptions options)
    {
        UiSession.Check(!options.AllowMultiple, "unexpected picker mode");
        UiSession.Check(choices.Count > 0, "unscripted picker request");
        OpenCalls++;
        return Task.FromResult(choices.Dequeue());
    }
    public Task<IStorageFile?> SaveFilePickerAsync(FilePickerSaveOptions options) => throw Forbidden();
    public Task<SaveFilePickerResult> SaveFilePickerWithResultAsync(FilePickerSaveOptions options) => throw Forbidden();
    public Task<IReadOnlyList<IStorageFolder>> OpenFolderPickerAsync(FolderPickerOpenOptions options) => throw Forbidden();
    public Task<IStorageBookmarkFile?> OpenFileBookmarkAsync(string bookmark) => throw Forbidden();
    public Task<IStorageBookmarkFolder?> OpenFolderBookmarkAsync(string bookmark) => throw Forbidden();
    public Task<IStorageFile?> TryGetFileFromPathAsync(Uri path) => throw Forbidden();
    public Task<IStorageFolder?> TryGetFolderFromPathAsync(Uri path) => throw Forbidden();
    public Task<IStorageFolder?> TryGetWellKnownFolderAsync(WellKnownFolder folder) => throw Forbidden();
    private static Exception Forbidden() => new InvalidOperationException("non-open storage operation forbidden");
}

internal sealed class TestStorageFile(string name) : IStorageFile
{
    public string Name { get; } = name;
    public Uri Path => new("file:///zipelo-harness-only/" + Uri.EscapeDataString(Name));
    public bool CanBookmark => false;
    public int Disposals { get; private set; }
    public Task<Stream> OpenReadAsync()
    {
        UiSession.Check(Disposals == 0, "disposed selection reopened");
        return Task.FromResult<Stream>(new MemoryStream((byte[])Fixture.Bytes.Clone(), writable: false));
    }
    public void Dispose() => Disposals++;
    public Task<StorageItemProperties> GetBasicPropertiesAsync() => Task.FromResult(new StorageItemProperties((ulong)Fixture.Bytes.Length));
    public Task<string?> SaveBookmarkAsync() => throw Forbidden();
    public Task<IStorageFolder?> GetParentAsync() => throw Forbidden();
    public Task DeleteAsync() => throw Forbidden();
    public Task<IStorageItem?> MoveAsync(IStorageFolder destination) => throw Forbidden();
    public Task<Stream> OpenWriteAsync() => throw Forbidden();
    private static Exception Forbidden() => new InvalidOperationException("fixture is read-only");
}
