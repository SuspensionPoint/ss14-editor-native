using System.Diagnostics;
using System.Text;
using System.Text.Json;

namespace SS14.EditorNative;

/// <summary>
/// Checks what the kernel says about prototypes against facts of the game that don't change. A
/// wall is tagged as a wall. The decals a mapper uses are listed with their pictures and with the
/// settings the game's own decal palette starts each one with. A tile says whether it can be
/// turned.
/// </summary>
public static class FactsCheck
{
    /// <summary>A decal prototype's switches, each with what the game gives it when a prototype says nothing.</summary>
    private static readonly (string Name, bool Default)[] Switches =
    [
        ("showMenu", true), ("snapCardinals", false), ("defaultCustomColor", false), ("defaultSnap", true), ("defaultCleanable", false),
    ];

    /// <returns>0 if every fact held.</returns>
    public static int Run(string forkRepo, string? kernelPack, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, say);
        CheckTimes.Report("facts", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, Action<string> say)
    {
        var failed = 0;
        void Expect(bool holds, string what)
        {
            say($"  {(holds ? "ok  " : "FAIL")}  {what}");
            if (!holds)
                failed++;
        }

        // A question the kernel cannot answer is a fact that did not hold.
        void Ask(string about, Action facts)
        {
            say(about);
            try
            {
                facts();
            }
            catch (Exception e)
            {
                Expect(false, e.Message);
            }
        }

        Ask("entity prototypes", () =>
        {
            var asked = Stopwatch.StartNew();
            var json = session.Command("prototypes", "{}");
            var firstMs = asked.ElapsedMilliseconds;
            asked.Restart();
            session.Command("prototypes", "{}");
            var againMs = asked.ElapsedMilliseconds;

            using var prototypes = JsonDocument.Parse(json);
            var all = prototypes.RootElement.EnumerateArray().ToArray();
            say($"  {all.Length} listed in {Encoding.UTF8.GetByteCount(json)} bytes, answered in {firstMs} ms and again in {againMs} ms");
            JsonElement Prototype(string id) => all.FirstOrDefault(prototype => Text(prototype, "id") == id);

            var wallTags = Strings(Prototype("WallSolid"), "tags");
            Expect(wallTags?.Contains("Wall") == true, $"WallSolid is tagged Wall (its tags: {Listed(wallTags)})");

            var diagonalTags = Strings(Prototype("WallReinforcedDiagonal"), "tags");
            Expect(diagonalTags is ["Diagonal", "Wall"], $"WallReinforcedDiagonal is tagged Diagonal and Wall and nothing else (its tags: {Listed(diagonalTags)})");

            // Nothing to say is an empty list, which a reader can count on being there.
            var without = all.Where(prototype => Strings(prototype, "components")?.Contains("Tag") != true).ToArray();
            var empty = without.Count(prototype => Strings(prototype, "tags") is { Length: 0 });
            Expect(without.Length > 0 && empty == without.Length, $"{empty} of the {without.Length} with no Tag component have an empty list of tags");

            var unordered = all.Where(prototype => Strings(prototype, "tags") is not { } tags || !Ascending(tags)).Select(prototype => Text(prototype, "id") ?? "").ToArray();
            Expect(unordered.Length == 0, unordered.Length == 0
                ? "every list of tags is in the order of its letters, none twice"
                : $"{unordered.Length} lists of tags are not in the order of their letters: {Listed(unordered.Take(8).ToArray())}");

            // A wall's Transform says anchored: true. A crate and a crowbar say nothing of it.
            string Anchored(string id) => Flag(Prototype(id), "anchored") is { } value ? value ? "true" : "false" : "not told";
            Expect(Flag(Prototype("WallSolid"), "anchored") == true, $"WallSolid is anchored (anchored {Anchored("WallSolid")})");
            Expect(Flag(Prototype("CrateGenericSteel"), "anchored") == false && Flag(Prototype("Crowbar"), "anchored") == false,
                $"CrateGenericSteel and Crowbar are not (anchored {Anchored("CrateGenericSteel")} and {Anchored("Crowbar")})");
            var anchored = all.Where(prototype => Flag(prototype, "anchored") == true).Select(prototype => Text(prototype, "id") ?? "").ToArray();
            Expect(anchored.Length > 0 && anchored.Length * 2 < all.Length,
                $"{anchored.Length} of the {all.Length} are anchored, some and fewer than half, among them {Listed(anchored.Where((_, index) => index % 500 == 0).Take(6).ToArray())}");

            // A disposal tube's exits as Direction numbers (South 0, East 2, North 4, West 6), in the
            // fork's order; a trunk keeps the component's default, south. Others have none.
            string Exits(string id) => Prototype(id).TryGetProperty("tubeExits", out var exits) && exits.ValueKind == JsonValueKind.Array
                ? "[" + string.Join(", ", exits.EnumerateArray().Select(exit => exit.GetInt32())) + "]"
                : "none";
            Expect((Exits("DisposalPipe"), Exits("DisposalTrunk"), Exits("DisposalJunction"), Exits("WallSolid")) == ("[0, 4]", "[0]", "[0, 6, 4]", "none"),
                $"disposal exits: DisposalPipe {Exits("DisposalPipe")}, DisposalTrunk {Exits("DisposalTrunk")}, DisposalJunction {Exits("DisposalJunction")}, WallSolid {Exits("WallSolid")}");
        });

        Ask("decal prototypes", () =>
        {
            using var decals = JsonDocument.Parse(session.Command("decals", "{}"));
            var all = decals.RootElement.EnumerateArray().ToArray();
            Expect(all.Length > 100, $"{all.Length} decal prototypes listed");
            JsonElement Decal(string id) => all.FirstOrDefault(decal => Text(decal, "id") == id);

            var ids = all.Select(decal => Text(decal, "id") ?? "").ToArray();
            Expect(Ascending(ids), "in the order of their ids, none twice");

            // The editor's store of textures is keyed by the names the files have, so a picture
            // spelled in another letter case is a picture it does not find.
            var roots = new[] { Path.Combine(forkRepo, "Resources"), SS14.EditorKernel.ResourcePack.EngineResources(forkRepo) };
            var listings = new Dictionary<string, HashSet<string>>();
            var files = all.Select(decal => (Path: Text(decal, "spritePath"), State: Text(decal, "spriteState")))
                .Select(picture => picture.Path == null ? null : picture.State == null ? picture.Path : $"{picture.Path}/{picture.State}.png")
                .ToArray();
            var missing = ids.Where((_, index) => files[index] is not { } file || !roots.Any(root => IsFile(root, file, listings))).ToArray();
            var states = all.Count(decal => Text(decal, "spriteState") != null);
            Expect(missing.Length == 0, missing.Length == 0
                ? $"every one has a picture, a file of the fork letter for letter ({states} a state of an RSI, {all.Length - states} an image)"
                : $"{missing.Length} have no picture among the fork's files, letter for letter: {Listed(missing.Take(8).ToArray())}");

            var tagged = all.Count(decal => Strings(decal, "tags") is { Length: > 0 });
            Expect(tagged > 0, $"{tagged} have tags");

            var arrows = Decal("Arrows");
            Expect(Text(arrows, "spritePath") == "/Textures/Decals/markings.rsi" && Text(arrows, "spriteState") == "arrows",
                $"Arrows is the state arrows of /Textures/Decals/markings.rsi (told: {Text(arrows, "spriteState") ?? "no state"} of {Text(arrows, "spritePath") ?? "no path"})");
            Expect(Strings(arrows, "tags") is ["station", "markings"], $"Arrows is tagged station and then markings, the prototype's own order (its tags: {Listed(Strings(arrows, "tags"))})");
            Expect(Switches.All(item => Flag(arrows, item.Name) == item.Default), $"Arrows says nothing more, and has what the game then gives ({Told(arrows)})");

            var greyscale = Decal("ArrowsGreyscale");
            Expect(Flag(greyscale, "defaultCustomColor") == true, $"ArrowsGreyscale takes a colour unless told otherwise ({Told(greyscale)})");

            var grass = Decal("Grassa1");
            Expect(Flag(grass, "snapCardinals") == true && Flag(grass, "defaultSnap") == false, $"Grassa1 stays upright and is not put on the middle of a tile ({Told(grass)})");

            var dirt = Decal("Dirt");
            Expect(Flag(dirt, "defaultCleanable") == true, $"Dirt can be cleaned away unless told otherwise ({Told(dirt)})");

            var hidden = all.Count(decal => Flag(decal, "showMenu") == false);
            Expect(hidden > 0 && hidden * 2 < all.Length, $"{hidden} are kept out of the menu, some and fewer than half");
        });

        Ask("tiles", () =>
        {
            using var tiles = JsonDocument.Parse(session.Command("tiles", "{}"));
            var all = tiles.RootElement.EnumerateArray().ToArray();
            JsonElement Tile(string id) => all.FirstOrDefault(tile => Text(tile, "id") == id);
            string Told(string id) => Flag(Tile(id), "allowsRotation") is { } value ? value ? "true" : "false" : "not told";

            // Prototypes/Tiles/floors.yml: the first says allowRotationMirror: true, and the
            // second, like the tile it inherits from, says nothing of it.
            Expect(Flag(Tile("FloorSteelSlatsContinuous"), "allowsRotation") == true, $"FloorSteelSlatsContinuous can be turned and mirrored (allowsRotation {Told("FloorSteelSlatsContinuous")})");
            Expect(Flag(Tile("FloorSteel"), "allowsRotation") == false, $"FloorSteel cannot (allowsRotation {Told("FloorSteel")})");

            var turning = all.Count(tile => Flag(tile, "allowsRotation") == true);
            Expect(turning > 0 && turning * 4 < all.Length, $"{turning} of the {all.Length} tiles can be turned, some and fewer than a quarter");
        });

        Ask("the cleanup on delete", () =>
        {
            // ComponentEditor.Listing names components by their names. A fork that renamed one
            // is warned of at start.
            var unknown = session.OnDocumentKernel(kernel => kernel.UnknownListingComponents).ToArray();
            Expect(unknown.Length == 0, $"the fork knows every component the cleanup names (warned of: {Listed(unknown)})");
        });

        say(failed == 0 ? "\nThe kernel's facts hold." : $"\n{failed} did not hold.");
        return failed == 0 ? 0 : 1;
    }

    /// <summary>A property's strings, or null if it is not there or is not a list.</summary>
    private static string[]? Strings(JsonElement of, string name) =>
        of.ValueKind == JsonValueKind.Object && of.TryGetProperty(name, out var list) && list.ValueKind == JsonValueKind.Array
            ? list.EnumerateArray().Select(item => item.GetString() ?? "").ToArray()
            : null;

    /// <summary>A property's text, or null if it is not there or is null.</summary>
    private static string? Text(JsonElement of, string name) =>
        of.ValueKind == JsonValueKind.Object && of.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;

    /// <summary>A property's yes or no, or null if it is not there.</summary>
    private static bool? Flag(JsonElement of, string name) =>
        of.ValueKind == JsonValueKind.Object && of.TryGetProperty(name, out var value) && value.ValueKind is JsonValueKind.True or JsonValueKind.False ? value.GetBoolean() : null;

    private static string Listed(string[]? strings) => strings == null ? "not told" : strings.Length == 0 ? "none" : string.Join(", ", strings);

    /// <summary>A decal's switches as the kernel told them.</summary>
    private static string Told(JsonElement decal) =>
        string.Join(", ", Switches.Select(item => $"{item.Name} {(Flag(decal, item.Name) is { } value ? value ? "true" : "false" : "not told")}"));

    /// <summary>Whether each string comes after the one before it, letter by letter: sorted, and none twice.</summary>
    private static bool Ascending(string[] strings) => strings.Zip(strings.Skip(1)).All(pair => string.CompareOrdinal(pair.First, pair.Second) < 0);

    /// <summary>
    /// Whether a rooted resource path is a file under a Resources folder, spelled as it is on
    /// disk. Windows finds a file in any letter case, so every part of the path is looked for
    /// among the names its folder really holds.
    /// </summary>
    private static bool IsFile(string root, string rooted, Dictionary<string, HashSet<string>> listings)
    {
        var at = root;
        foreach (var part in rooted.Split('/', StringSplitOptions.RemoveEmptyEntries))
        {
            if (!listings.TryGetValue(at, out var names))
            {
                names = new HashSet<string>(StringComparer.Ordinal);
                if (Directory.Exists(at))
                    names.UnionWith(Directory.EnumerateFileSystemEntries(at).Select(entry => Path.GetFileName(entry)));
                listings[at] = names;
            }

            if (!names.Contains(part))
                return false;

            at = Path.Combine(at, part);
        }

        return File.Exists(at);
    }
}
