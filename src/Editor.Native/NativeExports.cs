using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// What a native host calls after loading the .NET runtime into its own process. Each export is a
/// plain function with C calling conventions, with no marshalling layer and no process boundary.
/// Text goes in as UTF-8 with a length. A reply comes back as UTF-8 in memory the caller frees
/// with <see cref="Free"/>. A draw list isn't copied at all, the caller gets its address.
///
/// The boundary is kept small. Replies are JSON for now. The hot paths could become fixed-layout
/// structures later.
/// </summary>
public static unsafe class NativeExports
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private static EngineSession? _session;
    private static NativeApi? _api;

    private static string Text(byte* text, int length) => Encoding.UTF8.GetString(text, length);

    private static byte* Reply(object value, int* length)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(value, Json);
        var memory = (byte*)NativeMemory.Alloc((nuint)bytes.Length);
        bytes.CopyTo(new Span<byte>(memory, bytes.Length));
        *length = bytes.Length;
        return memory;
    }

    private static byte* Guarded(int* length, Func<EngineSession, object> action)
    {
        try
        {
            if (_session == null)
                return Reply(new { ok = false, error = "The engine has not been started." }, length);
            return Reply(new { ok = true, result = action(_session) }, length);
        }
        catch (Exception e)
        {
            return Reply(new { ok = false, error = e.ToString() }, length);
        }
    }

    /// <summary>Starts both kernels and returns at once. 1 on success, 0 if it was already started.</summary>
    [UnmanagedCallersOnly]
    public static int Start(byte* forkRepo, int forkRepoLength, byte* kernelPack, int kernelPackLength)
    {
        if (_session != null)
            return 0;

        _session = new EngineSession(Text(forkRepo, forkRepoLength), kernelPackLength > 0 ? Text(kernelPack, kernelPackLength) : null, parallel: true);
        _api = new NativeApi(_session);
        return 1;
    }

    /// <summary>Waits until both kernels are up and reports how long each took.</summary>
    [UnmanagedCallersOnly]
    public static byte* WaitReady(int* length) => Guarded(length, session =>
    {
        session.Ready.GetAwaiter().GetResult();
        var warmUpMs = _warm ? 0 : _api!.WarmUp();
        _warm = true;
        return new
        {
            documentKernelMs = session.DocumentKernelReadyMs,
            viewKernelMs = session.ViewKernelReadyMs,
            bothMs = session.StartedAfterMs,
            warmUpMs,
            managedMb = GC.GetTotalMemory(false) / 1048576,
        };
    });

    private static bool _warm;

    /// <summary>
    /// One of the editor's commands (NativeApi.cs), JSON in and out. The reply is
    /// {"ok":true,"result":...} or {"ok":false,"error":"..."}.
    /// </summary>
    [UnmanagedCallersOnly]
    public static byte* Call(byte* op, int opLength, byte* args, int argsLength, int* length)
    {
        byte[] reply;
        try
        {
            if (_api == null)
                return Reply(new { ok = false, error = "The engine has not been started." }, length);

            var result = _api.Handle(Text(op, opLength), Text(args, argsLength));
            reply = Encoding.UTF8.GetBytes(string.Concat("{\"ok\":true,\"result\":", result, "}"));
        }
        catch (Exception e)
        {
            // What the kernel refuses (an edit that makes no sense) is said in one line.
            return Reply(new { ok = false, error = e is KernelException ? e.Message : e.ToString() }, length);
        }

        var memory = (byte*)NativeMemory.Alloc((nuint)reply.Length);
        reply.CopyTo(new Span<byte>(memory, reply.Length));
        *length = reply.Length;
        return memory;
    }

    /// <summary>Opens a map file and waits until its draw list is recorded.</summary>
    [UnmanagedCallersOnly]
    public static byte* OpenFile(byte* path, int pathLength, int* length) => Guarded(length, session =>
    {
        var file = Text(path, pathLength);
        var (scene, timing) = session.Open(File.ReadAllText(file), Path.GetFileName(file));
        var grid = scene.Grids.OrderByDescending(g => g.Tiles.Count).First();
        return new
        {
            document = scene.Document.Id,
            entities = scene.Entities.Count,
            grid = grid.Uid,
            // Where the grid's origin is in the world: tile (x, y) of the grid is drawn there plus (x, y).
            gridX = grid.X,
            gridY = grid.Y,
            // A tile to paint, so a host can try an edit without reading the scene.
            tileX = grid.Tiles[grid.Tiles.Count / 10 / 5 * 5],
            tileY = grid.Tiles[grid.Tiles.Count / 10 / 5 * 5 + 1],
            timing,
        };
    });

    /// <summary>Which tile to paint: a structure with the same layout on both sides, passed by address.</summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct TileEdit
    {
        public int Document;
        public int Grid;
        public int X;
        public int Y;
    }

    /// <summary>Paints one tile and waits until the changed draw list is recorded.</summary>
    [UnmanagedCallersOnly]
    public static byte* PaintTile(TileEdit* edit, byte* tile, int tileLength, int* length)
    {
        var (document, grid, x, y) = (edit->Document, edit->Grid, edit->X, edit->Y);
        var name = Text(tile, tileLength);
        return Guarded(length, session => session.Edit(document, "Paint tile", [new EditOp("tile", Grid: grid, X: x, Y: y, Tile: name)]).Timing);
    }

    /// <summary>
    /// The layer switches. Flags: 1 entities, 2 markers, 4 decals, 8 what lies under floors.
    /// <paramref name="hiddenDepths"/> is pairs of lowest and highest draw depth to hide.
    /// Waits until the changed draw list is recorded.
    /// </summary>
    [UnmanagedCallersOnly]
    public static byte* SetLayers(int document, int flags, int* hiddenDepths, int hiddenCount, int* length)
    {
        var hidden = new ReadOnlySpan<int>(hiddenDepths, hiddenCount).ToArray();
        return Guarded(length, session => session.SetLayers(document, (flags & 1) != 0, (flags & 2) != 0, (flags & 4) != 0, hidden, (flags & 8) != 0));
    }

    /// <summary>
    /// The latest draw-list message for a document, where the engine wrote it. Valid until the
    /// next message for that document is recorded. Returns its length, or 0 if there is none.
    /// </summary>
    [UnmanagedCallersOnly]
    public static int DrawList(int document, byte** address)
    {
        var (where, length) = _session?.LatestDrawList(document) ?? (IntPtr.Zero, 0);
        *address = (byte*)where;
        return length;
    }

    [UnmanagedCallersOnly]
    public static byte* Save(int document, int* length) => Guarded(length, session =>
    {
        var saved = session.Save(document);
        return new { saved.Ok, saved.Failure, saved.Milliseconds, bytes = saved.Yaml?.Length ?? 0 };
    });

    /// <summary>One of the document kernel's named commands, JSON in and out.</summary>
    [UnmanagedCallersOnly]
    public static byte* Command(byte* op, int opLength, byte* args, int argsLength, int* length)
    {
        try
        {
            if (_session == null)
                return Reply(new { ok = false, error = "The engine has not been started." }, length);

            var result = Encoding.UTF8.GetBytes(_session.Command(Text(op, opLength), Text(args, argsLength)));
            var memory = (byte*)NativeMemory.Alloc((nuint)result.Length);
            result.CopyTo(new Span<byte>(memory, result.Length));
            *length = result.Length;
            return memory;
        }
        catch (Exception e)
        {
            return Reply(new { ok = false, error = e.Message }, length);
        }
    }

    [UnmanagedCallersOnly]
    public static void Free(byte* memory) => NativeMemory.Free(memory);
}
