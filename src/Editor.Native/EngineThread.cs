using System.Collections.Concurrent;

namespace SS14.EditorNative;

/// <summary>
/// One engine world on its own thread. The engine keeps its services per thread, so each world is
/// started on a dedicated thread and everything asked of it runs there, in order.
/// </summary>
public sealed class EngineThread<T> where T : class
{
    private readonly BlockingCollection<Action<T>> _queue = new();
    private readonly TaskCompletionSource<T> _ready = new(TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>Milliseconds from creating this object until the world was ready.</summary>
    public long ReadyAfterMs { get; private set; }

    public string Progress { get; private set; } = "Starting";

    public EngineThread(string name, Func<Action<string>, T> start)
    {
        var created = System.Diagnostics.Stopwatch.StartNew();
        var thread = new Thread(() =>
        {
            T world;
            try
            {
                world = start(message => Progress = message);
                ReadyAfterMs = created.ElapsedMilliseconds;
                _ready.SetResult(world);
            }
            catch (Exception e)
            {
                _ready.SetException(e);
                return;
            }

            foreach (var work in _queue.GetConsumingEnumerable())
                work(world);
        })
        {
            IsBackground = true,
            Name = name,
        };
        thread.Start();
    }

    public Task<T> Ready => _ready.Task;

    /// <summary>Queues work for the world's thread and returns at once.</summary>
    public void Post(Action<T> work) => _queue.Add(work);

    /// <summary>Runs work on the world's thread and waits for its result.</summary>
    public R Run<R>(Func<T, R> work)
    {
        _ready.Task.GetAwaiter().GetResult();
        var done = new TaskCompletionSource<R>(TaskCreationOptions.RunContinuationsAsynchronously);
        _queue.Add(world =>
        {
            try
            {
                done.SetResult(work(world));
            }
            catch (Exception e)
            {
                done.SetException(e);
            }
        });
        return done.Task.GetAwaiter().GetResult();
    }
}
