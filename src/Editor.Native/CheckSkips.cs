namespace SS14.EditorNative;

/// <summary>
/// Steps a check couldn't take because the map lacks what they need, like a second grid, a decal
/// of a kind the game doesn't have, or a button linked to two things. Each one prints a line
/// starting with "skip" and is counted, so a skipped step is never mistaken for a pass. A skip
/// doesn't fail the check.
/// </summary>
public static class CheckSkips
{
    private static int _count;

    /// <summary>How many steps were skipped in this process so far.</summary>
    public static int Count => _count;

    public static void Skip(Action<string> say, string text)
    {
        Interlocked.Increment(ref _count);
        say($"  skip  {text}");
    }

    /// <summary>The closing line of a check that skipped steps since <paramref name="since"/>, or none.</summary>
    public static void Report(Action<string> say, int since)
    {
        var skipped = _count - since;
        if (skipped > 0)
            say($"{skipped} steps skipped: the maps lack what they need. A skipped step is no pass.");
    }
}
