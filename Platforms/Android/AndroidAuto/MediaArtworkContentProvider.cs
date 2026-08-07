using Android.Content;
using Android.Database;
using Android.Net;
using Android.OS;
using Java.IO;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using AndroidUri = Android.Net.Uri;
using SystemUri = System.Uri;

namespace mashin.Platforms.Android.AndroidAuto;

[ContentProvider(
    new[] { Authority },
    Name = Name,
    Exported = true,
    GrantUriPermissions = true)]
public sealed class MediaArtworkContentProvider : ContentProvider
{
    private const string LogTag = "mashin.ArtworkProvider";

    public const string Authority = "com.companyname.mashin.mediaart";
    public const string Name = "com.companyname.mashin.MediaArtworkContentProvider";

    private const string QuerySource = "src";
    private static readonly HttpClient HttpClient = new();

    public override bool OnCreate() => true;

    public override string? GetType(AndroidUri uri) => "image/*";

    public static AndroidUri BuildContentUri(string sourceUrl)
    {
        var encodedSource = AndroidUri.Encode(sourceUrl);
        return new AndroidUri.Builder()
            .Scheme(ContentResolver.SchemeContent)
            .Authority(Authority)
            .AppendPath("art")
            .AppendQueryParameter(QuerySource, encodedSource)
            .Build();
    }

    public override ParcelFileDescriptor? OpenFile(AndroidUri uri, string? mode)
    {
        try
        {
            var context = Context;
            if (context == null)
            {
                return null;
            }

            var sourceParam = uri.GetQueryParameter(QuerySource);
            if (string.IsNullOrWhiteSpace(sourceParam))
            {
                return null;
            }

            var sourceUrl = AndroidUri.Decode(sourceParam);
            if (!SystemUri.TryCreate(sourceUrl, System.UriKind.Absolute, out var source))
            {
                return null;
            }

            var cacheDir = new System.IO.DirectoryInfo(System.IO.Path.Combine(context.CacheDir?.AbsolutePath ?? string.Empty, "media-art"));
            if (!cacheDir.Exists)
            {
                cacheDir.Create();
            }

            var extension = ResolveExtension(sourceUrl, source);
            if (string.IsNullOrWhiteSpace(extension) || extension.Length > 8)
            {
                extension = ".img";
            }

            var cacheFileName = ComputeSha256(sourceUrl) + extension;
            var localPath = System.IO.Path.Combine(cacheDir.FullName, cacheFileName);

            if (!System.IO.File.Exists(localPath)
                || new System.IO.FileInfo(localPath).Length == 0)
            {
                if (string.Equals(source.Scheme, SystemUri.UriSchemeHttp, StringComparison.OrdinalIgnoreCase)
                    || string.Equals(source.Scheme, SystemUri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase))
                {
                    DownloadToFile(source, localPath);
                }
                else if (string.Equals(source.Scheme, "data", StringComparison.OrdinalIgnoreCase))
                {
                    WriteDataUriToFile(sourceUrl, localPath);
                }
                else
                {
                    return null;
                }
            }

            if (!System.IO.File.Exists(localPath))
            {
                return null;
            }

            var javaFile = new Java.IO.File(localPath);
            return ParcelFileDescriptor.Open(javaFile, ParcelFileMode.ReadOnly);
        }
        catch
        {
            return null;
        }
    }

    private static void DownloadToFile(SystemUri source, string targetPath)
    {
        using var response = HttpClient.GetAsync(source, HttpCompletionOption.ResponseHeadersRead).GetAwaiter().GetResult();
        response.EnsureSuccessStatusCode();

        var tempPath = targetPath + ".tmp";
        using (var sourceStream = response.Content.ReadAsStream())
        using (var targetStream = System.IO.File.Create(tempPath))
        {
            sourceStream.CopyTo(targetStream);
        }

        if (System.IO.File.Exists(targetPath))
        {
            System.IO.File.Delete(targetPath);
        }

        System.IO.File.Move(tempPath, targetPath);
    }

    private static string ResolveExtension(string sourceUrl, SystemUri source)
    {
        if (string.Equals(source.Scheme, "data", StringComparison.OrdinalIgnoreCase))
        {
            var commaIndex = sourceUrl.IndexOf(',', StringComparison.Ordinal);
            var header = commaIndex > 0 ? sourceUrl[..commaIndex] : string.Empty;

            if (header.Contains("image/jpeg", StringComparison.OrdinalIgnoreCase)
                || header.Contains("image/jpg", StringComparison.OrdinalIgnoreCase))
            {
                return ".jpg";
            }

            if (header.Contains("image/png", StringComparison.OrdinalIgnoreCase))
            {
                return ".png";
            }

            if (header.Contains("image/webp", StringComparison.OrdinalIgnoreCase))
            {
                return ".webp";
            }

            return ".img";
        }

        var extension = System.IO.Path.GetExtension(source.AbsolutePath);
        return string.IsNullOrWhiteSpace(extension) ? ".img" : extension;
    }

    private static void WriteDataUriToFile(string sourceUrl, string targetPath)
    {
        var commaIndex = sourceUrl.IndexOf(',', StringComparison.Ordinal);
        if (commaIndex <= 0 || commaIndex >= sourceUrl.Length - 1)
        {
            return;
        }

        var header = sourceUrl[..commaIndex];
        var payload = sourceUrl[(commaIndex + 1)..];
        var isBase64 = header.Contains(";base64", StringComparison.OrdinalIgnoreCase);

        byte[] bytes;
        if (isBase64)
        {
            bytes = System.Convert.FromBase64String(payload);
        }
        else
        {
            var unescaped = SystemUri.UnescapeDataString(payload);
            bytes = Encoding.UTF8.GetBytes(unescaped);
        }

        var tempPath = targetPath + ".tmp";
        System.IO.File.WriteAllBytes(tempPath, bytes);

        if (System.IO.File.Exists(targetPath))
        {
            System.IO.File.Delete(targetPath);
        }

        System.IO.File.Move(tempPath, targetPath);
    }

    private static string ComputeSha256(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var hash = SHA256.HashData(bytes);
        var builder = new StringBuilder(hash.Length * 2);
        foreach (var b in hash)
        {
            builder.Append(b.ToString("x2"));
        }

        return builder.ToString();
    }

    public override ICursor? Query(AndroidUri uri, string[]? projection, string? selection, string[]? selectionArgs, string? sortOrder) => null;

    public override AndroidUri? Insert(AndroidUri uri, ContentValues? values) => null;

    public override int Update(AndroidUri uri, ContentValues? values, string? selection, string[]? selectionArgs) => 0;

    public override int Delete(AndroidUri uri, string? selection, string[]? selectionArgs) => 0;
}
