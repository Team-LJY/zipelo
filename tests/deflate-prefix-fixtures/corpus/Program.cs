using System.Text.Json;
using Zipelo.PrefixFixtures;

if (args.Length > 1 || (args.Length == 1 && args[0] != "--emit"))
{
    Console.Error.WriteLine("사용법: Zipelo.PrefixCorpus [--emit]");
    return 2;
}
try
{
    var fixtures = Fixtures.Build();
    if (args.Length == 1)
    {
        foreach (var fixture in fixtures) { Console.WriteLine(JsonSerializer.Serialize(fixture, Specification.Json)); }
    }
    else
    {
        Console.WriteLine("spec=7 valid=4 invalid=3; decoder 미실행");
    }
    return 0;
}
catch (Exception error)
{
    Console.Error.WriteLine(error.GetType().Name + ": " + error.Message);
    return 1;
}
