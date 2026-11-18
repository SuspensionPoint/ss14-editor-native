using System.Diagnostics;

namespace SS14.EditorNative;

/// <summary>
/// Where a check's time goes, by stage. A check wraps what it wants timed in <see cref="In"/>, and
/// <see cref="Report"/> prints the stages since the last report, with the rest of the wall time
/// as "everything else". A stage nested inside another counts only toward the inner one.
/// </summary>
public static class CheckTimes
{
    public const string Boot = "engine boot";
    public const string Save = "save";
    public const string Text = "saved text written and split";
    public const string Compare = "compare text";
    public const string Edit = "edit, undo and redo";
    public const string Reopen = "open a saved file again";

    public static string Open(string map) => $"open {Path.GetFileName(map)}";

    public sealed class Scope : IDisposable
    {
        internal string Stage = "";
        internal long Started;
        internal long Inner;
        internal Scope? Outer;

        public void Dispose()
        {
            var took = Stopwatch.GetTimestamp() - Started;
            _current = Outer;
            if (Outer != null)
                Outer.Inner += took;
            lock (Stages)
            {
                var index = Stages.FindIndex(stage => stage.Name == Stage);
                if (index < 0)
                    Stages.Add((Stage, took - Inner, 1));
                else
                    Stages[index] = (Stage, Stages[index].Ticks + took - Inner, Stages[index].Count + 1);
            }
        }
    }

    private static readonly List<(string Name, long Ticks, int Count)> Stages = new();
    private static readonly List<(string Check, double Seconds, List<(string Name, long Ticks, int Count)> Stages)> Reported = new();
    private static long _since = Stopwatch.GetTimestamp();

    // A stage belongs to the thread that began it. The engine's thread and the caller's each have their own.
    [ThreadStatic] private static Scope? _current;

    public static Scope In(string stage)
    {
        var scope = new Scope { Stage = stage, Started = Stopwatch.GetTimestamp(), Outer = _current };
        _current = scope;
        return scope;
    }

    /// <summary>Prints the stages of one check, and begins the count for the next.</summary>
    public static void Report(string check, Action<string> say)
    {
        List<(string Name, long Ticks, int Count)> stages;
        lock (Stages)
        {
            stages = Stages.ToList();
            Stages.Clear();
        }

        var now = Stopwatch.GetTimestamp();
        var wall = Seconds(now - _since);
        _since = now;
        Reported.Add((check, wall, stages));

        say($"\ntime in {check}: {wall:0.0} s");
        foreach (var (name, ticks, count) in stages)
            say($"  {name,-32}{Seconds(ticks),8:0.0} s{(count > 1 ? $"  ({count} times, {Seconds(ticks) * 1000 / count:0} ms each)" : "")}");
        say($"  {"everything else",-32}{wall - stages.Sum(stage => Seconds(stage.Ticks)),8:0.0} s");
        say($"  peak memory so far {PeakMb()} MB");
    }

    /// <summary>One line for each check reported, and the whole process.</summary>
    public static void Summary(Action<string> say)
    {
        using var process = Process.GetCurrentProcess();
        say("");
        foreach (var (check, seconds, stages) in Reported)
        {
            double Of(Func<string, bool> which) => stages.Where(stage => which(stage.Name)).Sum(stage => Seconds(stage.Ticks));
            say($"  {check,-8}{seconds,7:0.0} s: boot {Of(name => name == Boot):0.0}, open {Of(name => name.StartsWith("open ", StringComparison.Ordinal)):0.0}, "
                + $"save {Of(name => name == Save):0.0} ({stages.Where(stage => stage.Name == Save).Sum(stage => stage.Count)}), text {Of(name => name == Text):0.0}, "
                + $"compare {Of(name => name == Compare):0.0}, edits {Of(name => name == Edit):0.0}");
        }

        say($"whole process {(DateTime.Now - process.StartTime).TotalSeconds:0.0} s, peak memory {PeakMb()} MB");
    }

    private static double Seconds(long ticks) => ticks / (double) Stopwatch.Frequency;

    private static long PeakMb()
    {
        using var process = Process.GetCurrentProcess();
        return process.PeakWorkingSet64 / 1048576;
    }
}
