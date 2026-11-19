// The engine side of the native editor, run on its own to be measured.
//
//   bench <fork repo> [--cache <kernel pack .zip>] [--sequential] [--json <file>] [map path inside Resources ...]
//       Start both kernels in this process, open each map, paint tiles and place entities one at
//       a time, undo them, save, and report how long everything took and what it used.
//       --cache        start from pre-parsed prototypes (the pack `pack` below writes)
//       --sequential   start the view kernel only once the document kernel is up
//       --save-lists   write each map's complete draw list into this folder, to compare with
//                      the game client's
//
//   saves <fork repo> [--cache <kernel pack .zip>] [--rounds <n>] [map path inside Resources ...]
//       Save each map whole a few times, then step by step with a clock on each step, and
//       report where the time of a save goes (SaveBench.cs). Writes no file.
//
//   moves <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check that moving things keeps what refers to them: alarms' device links and what
//       containers hold (MoveCheck.cs). Writes the saves it compares into the folder.
//
//   regions <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check that a region of tiles, with what stands on it, moves, goes and turns as one
//       undo step and leaves the saved file right, and report what the engine does at the
//       edges of that (RegionCheck.cs). Writes the saves it compares into the folder.
//
//   copies <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check the kernel's copy and paste: a set of entities copied as text and pasted beside
//       the originals, twice, turned, where they stood after a cut, and on another map, with
//       undo, redo and the pastes the kernel must refuse (CopyCheck.cs). Writes the text and
//       the saves it compares into the folder.
//
//   keeps <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [--expect <Kind,Kind>] [map path inside Resources ...]
//       Open each map, save it with no edit, and count every kind of component in the file and
//       in the save (KeepsCheck.cs). A kind the save has fewer of fails: the engine side does not
//       know it. The kinds given with --expect must be in the file. Writes the saves into the folder.
//
//   facts <fork repo> [--cache <kernel pack .zip>]
//       Check what the kernel says about prototypes against facts of the game: a wall's tags,
//       and the decal prototypes with their pictures (FactsCheck.cs).
//
//   containers <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check the kernel's "contain" and "uncontain": a thing put into a locker and into a
//       slot, and one taken out, with the saved file read each time (ContainerCheck.cs).
//
//   links <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check the kernel's device links: its answers about links and ports, a link and a
//       listed device made and taken away, and linked things deleted (LinkCheck.cs).
//
//   grids <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check the kernel's "grid.add" and "grid.remove": a grid added, tiled, saved and
//       removed, and a grid of the map removed with all on it and put back. Also the map's
//       backdrop set, saved, read back and undone (GridCheck.cs).
//
//   airfix <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       Check the kernel's "air.fix", the game's fixgridatmos: every grid fixed with a marker of
//       each kind placed, the save read tile by tile, fixed twice, undone and redone, and what
//       the fix changed told by kind (AirFixCheck.cs).
//
//   visuals <fork repo> [--cache <kernel pack .zip>] [map path inside Resources ...]
//       Check the looks the kernel works out as the game's server would: pipe colours, cable
//       joins with a terminal and a cable laid, and what the view draws by them (VisualsCheck.cs).
//
//   saved <fork repo> [--cache <kernel pack .zip>] [map path inside Resources ...]
//       Check that the view shows each entity as the map saves it: the saved data and the
//       appearance values the kernel sends reach the view, valves, doors and filters are drawn
//       as their saved data says, and a valve closed by its data is drawn closed
//       (VisualsCheck.Saved.cs).
//
//   airsurvey <fork repo> [--cache <kernel pack .zip>] [map path inside Resources ...]
//       Fix each map and tell whether its stored air comes out as the file had it: the same
//       text, the same air in another order, or other air. Writes no file.
//
//   all <fork repo> [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       The checks above in turn on one engine, started once. Each opens the maps anew and
//       closes them, so none sees what another did. Their saves go into a folder each under
//       the folder given. Returns 1 if any of them failed.
//
//   quick [fork repo] [--cache <kernel pack .zip>] [--out <folder>] [map path inside Resources ...]
//       `all` on two small shuttles, which have what every step needs but for two things only
//       a station has (see quickMaps below). Run from the repository root it needs no argument: the
//       fork is upstream/starlight, the pack native/cache/kernel-resources.zip, and the
//       saves go into native/out/quick. It takes well under a minute. Run it while working.
//
//   pack <fork repo> <output zip>
//       Pack the fork's prototypes into a zip of pre-parsed documents. The editor runs
//       this to make a fork's pack for its cache (editor-core forks/cache.rs).
//
// Every check ends with where its time went (CheckTimes.cs). A step a map has not what it
// needs for is said in a line that begins "skip" and counted (CheckSkips.cs).
//
// The same assembly is what a Rust host loads; see NativeExports.cs.

using SS14.EditorNative;

// The maps of `quick`. Pursuit (600 entities) has alarms with their devices, buttons linked
// to one thing and to several, lockers that hold things, walls, loose things and decals. The
// second (370 entities) is there to paste into. Neither has a second grid or a decal of a kind
// the game does not have, so those two steps are skipped: only the stations test them.
string[] quickMaps = ["Maps/_Starlight/Shuttles/Security/pursuit.yml", "Maps/_Starlight/Shuttles/Shipyard/JSS_MED_Apotherkerin.yml"];

if (args.Length == 3 && args[0] == "pack")
{
    var (zip, prototypeFiles) = SS14.EditorKernel.ResourcePack.Build(Path.GetFullPath(args[1]));
    File.WriteAllBytes(args[2], zip);
    Console.WriteLine($"packed {prototypeFiles} prototype files into {args[2]} ({zip.Length / 1024.0 / 1024.0:F1} MB)");
    return 0;
}

if (args.Length >= 1 && args[0] == "quick")
{
    var quickArgs = args.Skip(1).ToList();
    if (quickArgs.Count == 0 || quickArgs[0].StartsWith("--", StringComparison.Ordinal) || quickArgs[0].EndsWith(".yml", StringComparison.OrdinalIgnoreCase))
        quickArgs.Insert(0, "upstream/starlight");
    if (!quickArgs.Contains("--cache") && File.Exists("native/cache/kernel-resources.zip"))
        quickArgs.AddRange(["--cache", "native/cache/kernel-resources.zip"]);
    if (!quickArgs.Contains("--out"))
        quickArgs.AddRange(["--out", "native/out/quick"]);
    if (!quickArgs.Any(arg => arg.EndsWith(".yml", StringComparison.OrdinalIgnoreCase)))
        quickArgs.AddRange(quickMaps);
    args = ["all", .. quickArgs];
}

if (args.Length < 2 || args[0] is not ("bench" or "saves" or "keeps" or "moves" or "regions" or "copies" or "facts" or "containers" or "links" or "grids" or "airfix" or "visuals" or "saved" or "airsurvey" or "all"))
{
    Console.Error.WriteLine("usage: Editor.Native bench <fork repo> [--cache <kernel pack>] [--sequential] [--json <file>] [map ...]");
    Console.Error.WriteLine("       Editor.Native saves <fork repo> [--cache <kernel pack>] [--rounds <n>] [map ...]");
    Console.Error.WriteLine("       Editor.Native moves <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native regions <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native copies <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native keeps <fork repo> [--cache <kernel pack>] [--out <folder>] [--expect <Kind,Kind>] [map ...]");
    Console.Error.WriteLine("       Editor.Native facts <fork repo> [--cache <kernel pack>]");
    Console.Error.WriteLine("       Editor.Native containers <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native links <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native grids <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native airfix <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native visuals <fork repo> [--cache <kernel pack>] [map ...]");
    Console.Error.WriteLine("       Editor.Native saved <fork repo> [--cache <kernel pack>] [map ...]");
    Console.Error.WriteLine("       Editor.Native airsurvey <fork repo> [--cache <kernel pack>] [map ...]");
    Console.Error.WriteLine("       Editor.Native all <fork repo> [--cache <kernel pack>] [--out <folder>] [map ...]");
    Console.Error.WriteLine("       Editor.Native quick [fork repo] [--cache <kernel pack>] [--out <folder>] [map ...]");
    return 2;
}

string? Option(string name)
{
    var at = Array.IndexOf(args, name);
    return at >= 0 && at + 1 < args.Length ? args[at + 1] : null;
}

var cache = Option("--cache");
var json = Option("--json");
var maps = args.Skip(2).Where(arg => arg.EndsWith(".yml", StringComparison.OrdinalIgnoreCase)).ToArray();
if (maps.Length == 0)
    maps = ["Maps/Lavaland/bait.yml"];

if (args[0] == "moves")
    return MoveCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-move-check"), Console.WriteLine);

if (args[0] == "regions")
    return RegionCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-region-check"), Console.WriteLine);

if (args[0] == "copies")
    return CopyCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-copy-check"), Console.WriteLine);

if (args[0] == "keeps")
{
    var expected = Option("--expect")?.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries) ?? [];
    return KeepsCheck.Run(args[1], cache, maps, expected, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-keeps-check"), Console.WriteLine);
}

if (args[0] == "facts")
    return FactsCheck.Run(args[1], cache, Console.WriteLine);

if (args[0] == "containers")
    return ContainerCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-container-check"), Console.WriteLine);

if (args[0] == "links")
    return LinkCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-link-check"), Console.WriteLine);

if (args[0] == "grids")
    return GridCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-grid-check"), Console.WriteLine);

if (args[0] == "airfix")
    return AirFixCheck.Run(args[1], cache, maps, Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-airfix-check"), Console.WriteLine);

if (args[0] == "visuals")
    return VisualsCheck.Run(args[1], cache, maps, Console.WriteLine);

if (args[0] == "saved")
    return VisualsCheck.RunSaved(args[1], cache, maps, Console.WriteLine);

if (args[0] == "airsurvey")
    return AirFixCheck.Survey(EngineSession.Booted(args[1], cache), args[1], maps, Console.WriteLine);

if (args[0] == "saves")
    return SaveBench.Run(args[1], cache, maps, int.TryParse(Option("--rounds"), out var rounds) ? rounds : 4, Console.WriteLine);

if (args[0] == "all")
{
    var folder = Option("--out") ?? Path.Combine(Path.GetTempPath(), "editor-checks");
    var session = EngineSession.Booted(args[1], cache);
    CheckTimes.Report("boot", Console.WriteLine);

    var results = new List<(string Check, bool Ok, int Skipped)>();
    void Check(string name, Func<int> run)
    {
        Console.WriteLine($"\n==== {name} ====");
        var skipsBefore = CheckSkips.Count;
        var ok = false;
        try
        {
            ok = run() == 0;
        }
        catch (Exception e)
        {
            Console.WriteLine($"  FAIL  {name} stopped: {e.Message}");
        }

        results.Add((name, ok, CheckSkips.Count - skipsBefore));
        CheckTimes.Report(name, Console.WriteLine);
    }

    Check("copies", () => CopyCheck.Run(session, args[1], maps, Path.Combine(folder, "copies"), Console.WriteLine));
    Check("regions", () => RegionCheck.Run(session, args[1], maps, Path.Combine(folder, "regions"), Console.WriteLine));
    Check("moves", () => MoveCheck.Run(session, args[1], maps, Path.Combine(folder, "moves"), Console.WriteLine));
    Check("facts", () => FactsCheck.Run(session, args[1], Console.WriteLine));
    Check("containers", () => ContainerCheck.Run(session, args[1], maps, Path.Combine(folder, "containers"), Console.WriteLine));
    Check("links", () => LinkCheck.Run(session, args[1], maps, Path.Combine(folder, "links"), Console.WriteLine));
    Check("grids", () => GridCheck.Run(session, args[1], maps, Path.Combine(folder, "grids"), Console.WriteLine));
    Check("airfix", () => AirFixCheck.Run(session, args[1], maps, Path.Combine(folder, "airfix"), Console.WriteLine));
    Check("visuals", () => VisualsCheck.Run(session, args[1], maps, Console.WriteLine));
    Check("saved", () => VisualsCheck.RunSaved(session, args[1], maps, Console.WriteLine));

    Console.WriteLine("\n==== all ====");
    foreach (var (name, ok, skipped) in results)
        Console.WriteLine($"  {(ok ? "ok  " : "FAIL")}  {name}{(skipped > 0 ? $", {skipped} steps skipped" : "")}");
    CheckTimes.Summary(Console.WriteLine);
    return results.All(result => result.Ok) ? 0 : 1;
}

var result = Bench.Run(args[1], cache, !args.Contains("--sequential"), maps, Console.WriteLine, Option("--save-lists"));
Console.WriteLine($"peak working set {result.PeakWorkingSetMb} MB");
if (json != null)
    File.WriteAllText(json, Bench.ToJson(result));
return 0;
