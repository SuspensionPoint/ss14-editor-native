using System.Numerics;
using Robust.Client.GameObjects;
using Robust.Client.Graphics;
using Robust.Client.ResourceManagement;
using Robust.Shared.GameObjects;
using Robust.Shared.Graphics;
using Robust.Shared.Graphics.RSI;
using Robust.Shared.IoC;
using Robust.Shared.Maths;
using Robust.Shared.Serialization.TypeSerializers.Implementations;
using Robust.Shared.Utility;

namespace Content.EditorView;

/// <summary>
/// A drawing handle that draws nothing. It writes down what it is asked to draw.
///
/// The engine's sprite code decides how an entity looks (which layers, which RSI state and
/// direction, what transform and tint) and then asks a drawing handle for textured rectangles.
/// Handing it this handle instead of the renderer's turns those decisions into plain data, which
/// any renderer (a WebGL canvas in a page, say) can draw without the engine.
/// </summary>
public sealed class DrawRecorder : DrawingHandleWorld
{
    /// <summary>Floats per recorded rectangle; see <see cref="Add"/> for the layout.</summary>
    public const int FloatsPerQuad = 17;

    private const float PixelsPerMeter = 32f;

    private readonly TextureRegistry _textures;
    private readonly SpriteSystem _sprites;
    private Matrix3x2 _transform = Matrix3x2.Identity;
    private ShaderInstance? _shader;

    /// <summary>Recorded rectangles since the last <see cref="Clear"/>.</summary>
    public readonly List<float> Quads = new();

    /// <summary>Draw requests that were not rectangles of a known texture, and so not recorded.</summary>
    public int Skipped;

    public DrawRecorder(TextureRegistry textures, SpriteSystem sprites) : base(Texture.White)
    {
        _textures = textures;
        _sprites = sprites;
    }

    public void Clear()
    {
        Quads.Clear();
        _transform = Matrix3x2.Identity;
        _shader = null;
    }

    public override void SetTransform(in Matrix3x2 matrix) => _transform = matrix;

    public override Matrix3x2 GetTransform() => _transform;

    public override void UseShader(ShaderInstance? shader) => _shader = shader;

    public override ShaderInstance? GetShader() => _shader;

    public override void DrawTextureRectRegion(Texture texture, Box2 quad, Color? modulate = null, UIBox2? subRegion = null)
    {
        Add(texture, quad.BottomLeft, quad.BottomRight, quad.TopRight, quad.TopLeft, modulate, subRegion);
    }

    public override void DrawTextureRectRegion(Texture texture, in Box2Rotated quad, Color? modulate = null, UIBox2? subRegion = null)
    {
        Add(texture, quad.BottomLeft, quad.BottomRight, quad.TopRight, quad.TopLeft, modulate, subRegion);
    }

#if ENGINE_DRAW_TEXTURE_RECTS
    // Only some engines draw in batches of rectangles (src/Directory.Build.props).
    public override void DrawTextureRects(Texture texture, ReadOnlySpan<WorldTextureRect> rects)
    {
        foreach (var rect in rects)
            DrawTextureRectRegion(texture, rect.Quad, rect.Modulate);
    }

    public override void DrawTextureRectsUnmodulated(Texture texture, ReadOnlySpan<WorldTextureRect> rects)
    {
        // Each rectangle keeps its own colour, only the handle's is left out (Clyde.Rendering.cs
        // DrawTextureBatch with applyModulation off).
        foreach (var rect in rects)
        {
            var quad = rect.Quad;
            Add(texture, quad.BottomLeft, quad.BottomRight, quad.TopRight, quad.TopLeft, rect.Modulate, null, modulated: false);
        }
    }
#endif

    public override void DrawTexture(Texture texture, Vector2 position, Color? modulate = null)
    {
        DrawTextureRectRegion(texture, Box2.FromDimensions(position, texture.Size / PixelsPerMeter), modulate);
    }

#if ENGINE_SPRITE_POST_SHADERS
    // Only some engines have post shaders on sprites.
    public override void RenderSpritePostShaders(
        Entity<SpriteComponent> sprite,
        IReadOnlyList<SpriteComponent.PostShaderEntry> postShaders,
        Angle eyeRotation,
        Angle worldRotation,
        Vector2 worldPosition,
        Direction? overrideDirection)
    {
        // Post-processing shaders (outlines and the like) are a renderer effect; record the
        // sprite as it is without them.
        _sprites.RenderSprite(sprite, this, eyeRotation, worldRotation, worldPosition, overrideDirection);
    }
#endif

    // Shapes and raw geometry are not something sprites use; they are counted, not recorded.

    public override void DrawPrimitives(DrawPrimitiveTopology primitiveTopology, Texture texture, ReadOnlySpan<DrawVertexUV2DColor> vertices) => Skipped++;

    public override void DrawPrimitives(DrawPrimitiveTopology primitiveTopology, Texture texture, ReadOnlySpan<ushort> indices, ReadOnlySpan<DrawVertexUV2DColor> vertices) => Skipped++;

    public override void DrawCircle(Vector2 position, float radius, Color color, bool filled = true) => Skipped++;

    public override void DrawLine(Vector2 from, Vector2 to, Color color) => Skipped++;

    public override void DrawRect(Box2 rect, Color color, bool filled = true) => Skipped++;

    public override void DrawRect(in Box2Rotated rect, Color color, bool filled = true) => Skipped++;

#if ENGINE_DRAW_RECTS
    public override void DrawRects(ReadOnlySpan<WorldRect> rects) => Skipped += rects.Length;

    public override void DrawRectsUnmodulated(ReadOnlySpan<WorldRect> rects) => Skipped += rects.Length;
#endif

    public override void RenderInRenderTarget(IRenderTarget target, Action a, Color? clearColor) => Skipped++;

    /// <summary>
    /// Records a rectangle directly: world-space corners in the order the texture's bottom-left,
    /// bottom-right, top-right and top-left go, for a texture known by id.
    /// </summary>
    public void AddRaw(int textureId, Vector2 bottomLeft, Vector2 bottomRight, Vector2 topRight, Vector2 topLeft, int width, int height)
    {
        Span<Vector2> corners = [bottomLeft, bottomRight, topRight, topLeft];
        foreach (var corner in corners)
        {
            Quads.Add(corner.X);
            Quads.Add(corner.Y);
        }

        Quads.AddRange([1f, 1f, 1f, 1f, textureId, 0f, 0f, width, height]);
    }

    /// <summary>
    /// One rectangle: its four corners in world space (bottom-left, bottom-right, top-right,
    /// top-left), tint, texture id, and the part of the texture to show in pixels (left, top,
    /// right, bottom). The tint is the colour given times the handle's own, unless
    /// <paramref name="modulated"/> is off.
    /// </summary>
    private void Add(Texture texture, Vector2 bottomLeft, Vector2 bottomRight, Vector2 topRight, Vector2 topLeft, Color? modulate, UIBox2? subRegion, bool modulated = true)
    {
        if (!_textures.TryGetId(texture, out var id))
        {
            Skipped++;
            return;
        }

        var color = modulated ? (modulate ?? Color.White) * Modulate : modulate ?? Color.White;
        // The engine marks an unshaded layer (one that ignores lighting) by sending its colour
        // as -1 - colour. Lighting is not part of what is recorded, so undo the marker.
        if (color.R < 0 || color.G < 0 || color.B < 0 || color.A < 0)
            color = new Color(-1 - color.R, -1 - color.G, -1 - color.B, -1 - color.A);

        Span<Vector2> corners = [bottomLeft, bottomRight, topRight, topLeft];
        foreach (var corner in corners)
        {
            var world = Vector2.Transform(corner, _transform);
            Quads.Add(world.X);
            Quads.Add(world.Y);
        }

        Quads.Add(color.R);
        Quads.Add(color.G);
        Quads.Add(color.B);
        Quads.Add(color.A);
        Quads.Add(id);

        var region = subRegion ?? new UIBox2(0, 0, texture.Width, texture.Height);
        Quads.Add(region.Left);
        Quads.Add(region.Top);
        Quads.Add(region.Right);
        Quads.Add(region.Bottom);
    }
}

/// <summary>Where a texture the engine draws with comes from, in terms a web page can load.</summary>
public sealed record TextureSource(
    int Id,
    /// <summary>Resource path of the RSI folder, or of the image itself when <see cref="State"/> is null.</summary>
    string Path,
    string? State,
    /// <summary>Which frame of the state's sheet, counting directions first and then frames.</summary>
    int Index,
    int Width,
    int Height);

/// <summary>
/// Gives every texture the engine draws with a small number and remembers which image file it
/// is. The engine hands the drawing handle texture objects, not file names, so the frames of an
/// RSI state are registered from the sprite's layers before the sprite is recorded.
/// </summary>
public sealed class TextureRegistry
{
    private readonly Dictionary<Texture, int> _ids = new(ReferenceEqualityComparer.Instance);
    private readonly HashSet<(RSI, RSI.StateId)> _registeredStates = new();
    private readonly List<TextureSource> _sources = new();
    private int _announced;

    public bool TryGetId(Texture texture, out int id) => _ids.TryGetValue(texture, out id);

    /// <summary>Textures registered since the last call.</summary>
    public IReadOnlyList<TextureSource> TakeNew()
    {
        var fresh = _sources.GetRange(_announced, _sources.Count - _announced);
        _announced = _sources.Count;
        return fresh;
    }

    /// <summary>Every texture registered so far, for a client that is starting from nothing.</summary>
    public IReadOnlyList<TextureSource> All => _sources;

    private readonly Dictionary<(string Path, int Index), int> _files = new();

    /// <summary>A frame of an image file, by path: for tiles, which are not sprites.</summary>
    public int File(string path, int index, int width, int height)
    {
        if (_files.TryGetValue((path, index), out var id))
            return id;

        id = _sources.Count;
        _files[(path, index)] = id;
        _sources.Add(new TextureSource(id, path, null, index, width, height));
        return id;
    }

    /// <summary>Registers what a sprite specifier (a decal's, say) will draw with.</summary>
    public void RegisterSpecifier(SpriteSpecifier specifier, SpriteSystem sprites, IResourceCache cache)
    {
        switch (specifier)
        {
            case SpriteSpecifier.Rsi rsi:
            {
                // An RSI or a state that is not there is drawn as the engine's error state
                // (SpriteSystem.GetState).
                var state = sprites.GetState(rsi);
                RegisterState(state.RSI, state);
                break;
            }
            case SpriteSpecifier.Texture texture:
            {
                var frame = sprites.Frame0(texture);
                if (!_ids.ContainsKey(frame))
                {
                    var id = _sources.Count;
                    _ids[frame] = id;
                    _sources.Add(new TextureSource(id, (SpriteSpecifierSerializer.TextureRoot / texture.TexturePath).ToString(), null, 0, frame.Width, frame.Height));
                }

                break;
            }
        }
    }

    /// <summary>
    /// Registers what a sprite's layers will draw with, as SpriteSystem.RenderLayer picks it: the
    /// layer's state as the engine resolved it, which is the error state when the RSI has no
    /// such state, or else the layer's own texture.
    /// </summary>
    public void RegisterLayers(SpriteComponent sprite, IResourceCache cache)
    {
        foreach (var item in sprite.AllLayers)
        {
            if (item is not SpriteComponent.Layer layer)
                continue;

            if (layer.ActualState is { } state)
                RegisterState(state.RSI, state);
            else if (layer.Texture is { } texture)
                RegisterTexture(texture, cache);
        }
    }

    /// <summary>The resource path of each texture loaded as a file, made when one is not found.</summary>
    private Dictionary<Texture, string>? _texturePaths;

    /// <summary>Textures with no file, so that they are not looked for again.</summary>
    private readonly HashSet<Texture> _unnamed = new(ReferenceEqualityComparer.Instance);

    /// <summary>
    /// A texture a layer was given whole, by its file. The layer keeps only the texture, so its
    /// file is found among the textures the resource cache has loaded.
    /// </summary>
    private void RegisterTexture(Texture texture, IResourceCache cache)
    {
        if (_ids.ContainsKey(texture) || _unnamed.Contains(texture))
            return;

        if (_texturePaths == null || !_texturePaths.ContainsKey(texture))
        {
            _texturePaths = new Dictionary<Texture, string>(ReferenceEqualityComparer.Instance);
            foreach (var (path, resource) in cache.GetAllResources<TextureResource>())
                _texturePaths.TryAdd(resource.Texture, path.ToString());
        }

        // A texture made in code has no file, and is left out as before.
        if (!_texturePaths.TryGetValue(texture, out var file))
        {
            _unnamed.Add(texture);
            return;
        }

        var id = _sources.Count;
        _ids[texture] = id;
        _sources.Add(new TextureSource(id, file, null, 0, texture.Width, texture.Height));
    }

    private void RegisterState(RSI rsi, RSI.State state)
    {
        if (!_registeredStates.Add((rsi, state.StateId)))
            return;

        var directions = state.RsiDirections switch
        {
            RsiDirectionType.Dir1 => 1,
            RsiDirectionType.Dir4 => 4,
            _ => 8,
        };

        // An RSI state's image holds its frames in reading order: every frame of the first
        // direction, then every frame of the next. When the directions' delays differ, the
        // engine folds them into one timeline that fits every direction (RSIResource.FoldDelays),
        // so a direction repeats an image frame for as long as it lasts. Each repeat is a texture
        // of its own, cut from the same place of the sheet, and the repeats come one after
        // another. So the count moves on only when the place a frame is cut from changes.
        var index = -1;
        for (var direction = 0; direction < directions; direction++)
        {
            Texture? previous = null;
            foreach (var frame in state.GetFrames((RsiDirection) direction))
            {
                if (previous == null || !SamePlace(previous, frame))
                    index++;
                previous = frame;

                if (!_ids.ContainsKey(frame))
                {
                    var id = _sources.Count;
                    _ids[frame] = id;
                    _sources.Add(new TextureSource(id, rsi.Path.ToString(), state.StateId.Name, index, frame.Width, frame.Height));
                }
            }
        }
    }

    /// <summary>Whether two frames are cut from the same place of the same sheet.</summary>
    private static bool SamePlace(Texture a, Texture b)
    {
        if (ReferenceEquals(a, b))
            return true;

        return a is AtlasTexture first && b is AtlasTexture second
            && ReferenceEquals(first.SourceTexture, second.SourceTexture)
            && first.SubRegion.Equals(second.SubRegion);
    }
}
