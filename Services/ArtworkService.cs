using ImageSharpImage = SixLabors.ImageSharp.Image;
using Microsoft.Extensions.Logging;
using Microsoft.Maui.Graphics;
using SixLabors.ImageSharp.Formats.Png;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;
using SixLabors.ImageSharp.Processing.Processors.Transforms;

namespace mashin.Services;

#region Interface

/// <summary>
/// Provides reusable artwork processing for covers, including accent color extraction and blurred imagery.
/// </summary>
public interface IArtworkService
{
    /// <summary>
    /// Calculates a representative accent color from cover image bytes.
    /// </summary>
    Task<Color?> GetAccentColorAsync(byte[]? imageBytes, CancellationToken cancellationToken = default);

    /// <summary>
    /// Returns a blurred image source for cover image bytes.
    /// </summary>
    Task<ImageSource?> GetBlurredCoverSourceAsync(byte[]? imageBytes, float blurRadius = 24f, CancellationToken cancellationToken = default);
}

#endregion

/// <summary>
/// Centralized artwork processing directly from in-memory image bytes.
/// </summary>
public sealed class ArtworkService : IArtworkService
{
    #region Fields

    private readonly ILogger<ArtworkService> _logger;

    #endregion

    #region Construction

    public ArtworkService(ILogger<ArtworkService> logger)
    {
        _logger = logger;
    }

    #endregion

    #region Public API

    public async Task<Color?> GetAccentColorAsync(byte[]? imageBytes, CancellationToken cancellationToken = default)
    {
        if (imageBytes == null || imageBytes.Length == 0)
        {
            return null;
        }

        try
        {
            var color = await Task.Run(() =>
            {
                using var sourceStream = new MemoryStream(imageBytes, writable: false);
                using var image = ImageSharpImage.Load<Rgba32>(sourceStream);

                image.Mutate(ctx =>
                {
                    ctx.Resize(new ResizeOptions
                    {
                        Mode = SixLabors.ImageSharp.Processing.ResizeMode.Max,
                        Size = new SixLabors.ImageSharp.Size(48, 48),
                        Sampler = KnownResamplers.Triangle,
                    });
                });

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
            }, cancellationToken);

            return color;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "ArtworkService: accent extraction failed for provided bytes.");
            return null;
        }
    }

    public async Task<ImageSource?> GetBlurredCoverSourceAsync(byte[]? imageBytes, float blurRadius = 24f, CancellationToken cancellationToken = default)
    {
        if (imageBytes == null || imageBytes.Length == 0)
        {
            return null;
        }

        try
        {
            var normalizedBlur = Math.Clamp(blurRadius, 1f, 64f);

            // Blur simulation via extreme downscale + upscale: resize to a tiny thumbnail,
            // then scale back up. The bilinear sampler smears all detail into a smooth
            // color wash — no GaussianBlur kernel needed, so this is very fast on Android.
            var blurredBytes = await Task.Run(() =>
            {
                using var sourceStream = new MemoryStream(imageBytes, writable: false);
                using var image = ImageSharpImage.Load<Rgba32>(sourceStream);

                image.Mutate(ctx =>
                {
                    ctx.Resize(new ResizeOptions
                    {
                        Mode = SixLabors.ImageSharp.Processing.ResizeMode.Stretch,
                        Size = new SixLabors.ImageSharp.Size(16, 16),
                        Sampler = KnownResamplers.Bicubic,
                    });
                    ctx.Resize(new ResizeOptions
                    {
                        Mode = SixLabors.ImageSharp.Processing.ResizeMode.Stretch,
                        Size = new SixLabors.ImageSharp.Size(200, 200),
                        Sampler = KnownResamplers.Bicubic,
                    });
                });

                using var outputStream = new MemoryStream();
                image.Save(outputStream, new SixLabors.ImageSharp.Formats.Jpeg.JpegEncoder { Quality = 70 });

                return outputStream.ToArray();
            }, cancellationToken);

            return ImageSource.FromStream(() => new MemoryStream(blurredBytes));
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "ArtworkService: blurred cover generation failed for provided bytes.");
            return null;
        }
    }

    #endregion

}
