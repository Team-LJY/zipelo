using System.Runtime.Loader;

namespace Zipelo.PrefixFixtures;

internal sealed class DecoderFactory
{
    private readonly Type inflater;
    private readonly Type formatError;
    public string Identity { get; }
    public string Sha256 { get; }

    public DecoderFactory(string path, string expectedHash)
    {
        path = Path.GetFullPath(path);
        Sha256 = Specification.Hash(File.ReadAllBytes(path));
        Specification.Require(Sha256.Equals(expectedHash, StringComparison.OrdinalIgnoreCase), "DLL SHA-256이 다름.");
        var assembly = AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
        inflater = assembly.GetType("ICSharpCode.SharpZipLib.Zip.Compression.Inflater", true)!;
        formatError = assembly.GetType("ICSharpCode.SharpZipLib.SharpZipBaseException", true)!;
        Specification.Require(inflater.Assembly == assembly && formatError.Assembly == assembly, "assembly 연결이 다름.");
        Identity = assembly.FullName ?? throw new InvalidDataException("assembly 이름이 없음.");
    }

    public Decoder Create() => new(inflater);
    public bool IsDeflateError(Exception error) => formatError.IsInstanceOfType(error);
}

internal sealed class Decoder
{
    private readonly Func<long> totalIn;
    private readonly Func<long> totalOut;
    private readonly Func<int> remaining;
    private readonly Func<bool> finished;
    private readonly Func<bool> needInput;
    private readonly Func<bool> needDictionary;
    public Action<byte[], int, int> SetInput { get; }
    public Func<byte[], int, int, int> Inflate { get; }
    public long TotalIn => totalIn();
    public long TotalOut => totalOut();
    public int Remaining => remaining();
    public bool Finished => finished();
    public bool NeedInput => needInput();
    public bool NeedDictionary => needDictionary();

    public Decoder(Type type)
    {
        var instance = Activator.CreateInstance(type, new object[] { true })
            ?? throw new InvalidDataException("raw inflater를 만들지 못함.");
        SetInput = Method<Action<byte[], int, int>>(type, instance, "SetInput");
        Inflate = Method<Func<byte[], int, int, int>>(type, instance, "Inflate");
        totalIn = Getter<long>(type, instance, "TotalIn");
        totalOut = Getter<long>(type, instance, "TotalOut");
        remaining = Getter<int>(type, instance, "RemainingInput");
        finished = Getter<bool>(type, instance, "IsFinished");
        needInput = Getter<bool>(type, instance, "IsNeedingInput");
        needDictionary = Getter<bool>(type, instance, "IsNeedingDictionary");
    }

    private static T Method<T>(Type type, object instance, string name) where T : Delegate =>
        (type.GetMethod(name, new[] { typeof(byte[]), typeof(int), typeof(int) })
            ?? throw new MissingMethodException(name)).CreateDelegate<T>(instance);

    private static Func<T> Getter<T>(Type type, object instance, string name) =>
        (type.GetProperty(name)?.GetGetMethod() ?? throw new MissingMemberException(name))
            .CreateDelegate<Func<T>>(instance);
}
