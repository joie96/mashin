using ImageSharpImage = SixLabors.ImageSharp.Image;
using Microsoft.Maui.Graphics;
using SixLabors.ImageSharp.Formats.Png;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;
using System.Collections.Concurrent;

namespace mashin.Services;

#region Interface

/// <summary>
/// Provides reusable artwork processing for covers, including accent color extraction and blurred imagery.
/// </summary>
public interface IArtworkService
{
    /// <summary>
    /// Calculates a representative accent color from a cover image URL.
    /// </summary>
    Task<Color?> GetAccentColorAsync(string? imageUrl, CancellationToken cancellationToken = default);

    /// <summary>
    /// Returns a blurred image source for a cover image URL.
    /// </summary>
    Task<ImageSource?> GetBlurredCoverSourceAsync(string? imageUrl, float blurRadius = 24f, CancellationToken cancellationToken = default);

    /// <summary>
    /// Derives a readable foreground/progress color from an accent color.
    /// </summary>
    Color GetForegroundColor(Color accentColor);
}

#endregion

/// <summary>
/// Centralized artwork processing with in-memory caching for downloads, accents, and blurred outputs.
/// </summary>
public sealed class ArtworkService : IArtworkService
{
    #region Fields

    private static readonly HttpClient ArtworkHttpClient = new() { Timeout = TimeSpan.FromSeconds(8) };

    private readonly ConcurrentDictionary<string, byte[]> _downloadCache = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, byte[]> _blurredCoverCache = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, Color> _accentColorCache = new(StringComparer.OrdinalIgnoreCase);

    #endregion

    #region Public API

    public async Task<Color?> GetAccentColorAsync(string? imageUrl, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(imageUrl) || !Uri.TryCreate(imageUrl, UriKind.Absolute, out _))
        {
            return null;
        }

        if (_accentColorCache.TryGetValue(imageUrl, out var cachedColor))
        {
            return cachedColor;
        }

        try
        {
            var bytes = await GetImageBytesAsync(imageUrl, cancellationToken);
            if (bytes.Length == 0)
            {
                return null;
            }

            await using var sourceStream = new MemoryStream(bytes);
            using var image = await ImageSharpImage.LoadAsync<Rgba32>(sourceStream, cancellationToken);

            // Keep color extraction fast by reducing image resolution before analysis.
            image.Mutate(ctx =>
            {
                ctx.Resize(new ResizeOptions
                {
                    Mode = SixLabors.ImageSharp.Processing.ResizeMode.Max,
                    Size = new SixLabors.ImageSharp.Size(48, 48),
                    Sampler = KnownResamplers.Triangle,
                });
            });

            var color = CalculateAccentColor(image);
            _accentColorCache[imageUrl] = color;

            return color;
        }
        catch
        {
            return null;
        }
    }

    public async Task<ImageSource?> GetBlurredCoverSourceAsync(string? imageUrl, float blurRadius = 24f, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(imageUrl) || !Uri.TryCreate(imageUrl, UriKind.Absolute, out _))
        {
            return null;
        }

        var normalizedBlur = Math.Clamp(blurRadius, 1f, 64f);
        var cacheKey = $"{imageUrl}::blur::{normalizedBlur:0.##}";

        if (_blurredCoverCache.TryGetValue(cacheKey, out var cachedBytes))
        {
            return ImageSource.FromStream(() => new MemoryStream(cachedBytes));
        }

        try
        {
            var bytes = await GetImageBytesAsync(imageUrl, cancellationToken);
            if (bytes.Length == 0)
            {
                return null;
            }

            await using var sourceStream = new MemoryStream(bytes);
            using var image = await ImageSharpImage.LoadAsync<Rgba32>(sourceStream, cancellationToken);

            // Gaussian blur is used so UI layers can reuse this as a soft backdrop.
            image.Mutate(ctx =>
            {
                ctx.GaussianBlur(normalizedBlur);
            });

            await using var outputStream = new MemoryStream();
            await image.SaveAsync(outputStream, new PngEncoder(), cancellationToken);

            var blurredBytes = outputStream.ToArray();
            _blurredCoverCache[cacheKey] = blurredBytes;

            return ImageSource.FromStream(() => new MemoryStream(blurredBytes));
        }
        catch
        {
            return null;
        }
    }

    public Color GetForegroundColor(Color accentColor)
    {
        var r = accentColor.Red;
        var g = accentColor.Green;
        var b = accentColor.Blue;

        var luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        // Bias toward a darker warm tone to keep progress fill visible on top of the accent bar.
        var fgR = Math.Clamp(r * 0.72 + 0.12, 0, 1);
        var fgG = Math.Clamp(g * 0.52 + 0.06, 0, 1);
        var fgB = Math.Clamp(b * 0.36 + 0.03, 0, 1);

        if (luminance < 0.28)
        {
            fgR = Math.Clamp(fgR + 0.16, 0, 1);
            fgG = Math.Clamp(fgG + 0.12, 0, 1);
            fgB = Math.Clamp(fgB + 0.08, 0, 1);
        }

        return Color.FromRgba(fgR, fgG, fgB, 0.92f);
    }

    #endregion

    #region Helpers

    private async Task<byte[]> GetImageBytesAsync(string imageUrl, CancellationToken cancellationToken)
    {
        if (_downloadCache.TryGetValue(imageUrl, out var cachedBytes))
        {
            return cachedBytes;
        }

        using var response = await ArtworkHttpClient.GetAsync(imageUrl, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
        response.EnsureSuccessStatusCode();

        var bytes = await response.Content.ReadAsByteArrayAsync(cancellationToken);
        _downloadCache[imageUrl] = bytes;

        return bytes;
    }

    // Produces a stable accent by weighting saturated mid-tone pixels stronger than extremes.
    private static Color CalculateAccentColor(SixLabors.ImageSharp.Image<Rgba32> image)
    {
        if (image.Width == 0 || image.Height == 0)
        {
            return Colors.Transparent;
        }

        double weightedR = 0;
        double weightedG = 0;
        double weightedB = 0;
        double totalWeight = 0;

        image.ProcessPixelRows(accessor =>
        {
            for (var y = 0; y < accessor.Height; y++)
            {
                var row = accessor.GetRowSpan(y);
                for (var x = 0; x < row.Length; x++)
                {
                    var pixel = row[x];
                    if (pixel.A == 0)
                    {
                        continue;
                    }

                    var r = pixel.R / 255d;
                    var g = pixel.G / 255d;
                    var b = pixel.B / 255d;

                    var max = Math.Max(r, Math.Max(g, b));
                    var min = Math.Min(r, Math.Min(g, b));
                    var chroma = max - min;
                    var luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;

                    // Favor saturated mid-tone colors to avoid muddy or near-black accents.
                    var weight = (0.35 + chroma) * (0.2 + Math.Clamp(1 - Math.Abs(0.55 - luminance), 0, 1));
                    if (weight <= 0)
                    {
                        continue;
                    }

                    weightedR += r * weight;
                    weightedG += g * weight;
                    weightedB += b * weight;
                    totalWeight += weight;
                }
            }
        });

        if (totalWeight <= 0.0001)
        {
            return Colors.Transparent;
        }

        var avgR = weightedR / totalWeight;
        var avgG = weightedG / totalWeight;
        var avgB = weightedB / totalWeight;

        var boost = 1.08;
        return Color.FromRgb(
            Math.Clamp(avgR * boost, 0, 1),
            Math.Clamp(avgG * boost, 0, 1),
            Math.Clamp(avgB * boost, 0, 1));
    }

    #endregion
}
