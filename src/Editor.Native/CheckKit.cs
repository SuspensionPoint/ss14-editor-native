using System.Text.Json;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// What the small checks (ContainerCheck, LinkCheck, GridCheck) share: opening and saving with the
/// time counted, comparing two saves, and a tally of what held.
/// </summary>
internal static class CheckKit
{
    /// <summary>Counts what held and what did not, and says each in one line.</summary>
    internal sealed class Tally(Action<string> say)
    {
        public int Failed { get; private set; }

        public bool Expect(bool holds, string what)
        {
            say($"  {(holds ? "ok  " : "FAIL")}  {what}");
            if (!holds)
                Failed++;
            return holds;
        }

        /// <summary>Two saves that must be the same file, with the first lines that are not.</summary>
        public bool Same(string[] expected, string[] actual, string what)
        {
            var differences = Differences(expected, actual);
            if (Expect(differences.Count == 0, differences.Count == 0 ? $"{what} ({expected.Length} lines)" : $"{what}: {differences.Count} differences"))
                return true;

            foreach (var line in differences.Take(8))
                say($"        {line}");
            return false;
        }
    }

    /// <summary>The check on each map in turn. A map that stops the check has failed.</summary>
    internal static int OnEach(IReadOnlyList<string> maps, Action<string> say, string passed, Func<string, Tally, bool> check)
    {
        var skipsBefore = CheckSkips.Count;
        var failed = 0;
        foreach (var map in maps)
        {
            say(map);
            var tally = new Tally(say);
            try
            {
                check(map, tally);
            }
            catch (Exception e)
            {
                tally.Expect(false, $"stopped: {e.Message}");
            }

            if (tally.Failed > 0)
                failed++;
        }

        say(failed == 0 ? $"\n{passed}" : $"\n{failed} of {maps.Count} maps failed.");
        CheckSkips.Report(say, skipsBefore);
        return failed == 0 ? 0 : 1;
    }

    internal static Scene Opened(EngineSession session, string stage, string yaml, string source)
    {
        using var timed = CheckTimes.In(stage);
        return session.Open(yaml, source).Scene;
    }

    internal static string[] Save(EngineSession session, int document, string path)
    {
        SaveResult saved;
        using (CheckTimes.In(CheckTimes.Save))
            saved = session.Save(document);
        if (!saved.Ok || saved.Yaml == null)
            throw new InvalidOperationException($"not saved: {saved.Failure}");

        using var timed = CheckTimes.In(CheckTimes.Text);
        File.WriteAllText(path, saved.Yaml);
        return saved.Yaml.Split('\n');
    }

    internal static SceneDelta Edit(EngineSession session, int document, string label, params EditOp[] ops)
    {
        using var timed = CheckTimes.In(CheckTimes.Edit);
        return session.Edit(document, label, ops).Delta;
    }

    internal static SceneDelta Undo(EngineSession session, int document)
    {
        using var timed = CheckTimes.In(CheckTimes.Edit);
        return session.Undo(document).Delta;
    }

    internal static SceneDelta Redo(EngineSession session, int document)
    {
        using var timed = CheckTimes.In(CheckTimes.Edit);
        return session.Redo(document).Delta;
    }

    /// <summary>What the kernel said as it refused an edit, or null if it did the edit.</summary>
    internal static string? Refusal(EngineSession session, int document, params EditOp[] ops)
    {
        try
        {
            Edit(session, document, "Refused", ops);
            return null;
        }
        catch (Exception e)
        {
            return (e as AggregateException)?.InnerException?.Message ?? e.Message;
        }
    }

    /// <summary>One of the kernel's questions, with its answer parsed.</summary>
    internal static JsonDocument Ask(EngineSession session, string command, string args) =>
        JsonDocument.Parse(session.Command(command, args));

    /// <summary>The lines in which two saves differ. The heading's line that says when is left out.</summary>
    internal static List<string> Differences(string[] before, string[] after)
    {
        using var timed = CheckTimes.In(CheckTimes.Compare);
        var found = new List<string>();
        if (before.Length != after.Length)
            found.Add($"{before.Length} lines became {after.Length}");

        for (var i = 0; i < Math.Min(before.Length, after.Length); i++)
        {
            if (before[i] == after[i])
                continue;

            if (i < 12 && before[i].TrimStart().StartsWith("time:", StringComparison.Ordinal) && after[i].TrimStart().StartsWith("time:", StringComparison.Ordinal))
                continue;

            found.Add($"line {i + 1}: \"{before[i].Trim()}\" became \"{after[i].Trim()}\"");
        }

        return found;
    }

    /// <summary>Whether a save names an entity that is not in it.</summary>
    internal static int Invalid(string[] lines) => lines.Count(line => line.Contains("invalid", StringComparison.Ordinal));
}
