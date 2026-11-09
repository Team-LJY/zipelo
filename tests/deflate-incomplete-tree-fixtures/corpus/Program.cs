using System.Text.Json;
using Zipelo.IncompleteFixtures;

if (args.Length > 1 || (args.Length == 1 && args[0] != "--emit"))
{
    Console.Error.WriteLine("사용법: Zipelo.IncompleteCorpus [--emit]");
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
        Console.WriteLine("spec=14 valid=6 invalid=8; decoder 미실행");
    }
    return 0;
}
catch (Exception error)
{
    Console.Error.WriteLine(error.GetType().Name + ": " + error.Message);
    return 1;
}
