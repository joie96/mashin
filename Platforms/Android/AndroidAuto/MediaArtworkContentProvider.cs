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
            if (!SystemUri.TryCreate(sourceUrl, System.UriKind.Absolute, out var source)
                || (source.Scheme != SystemUri.UriSchemeHttp && source.Scheme != SystemUri.UriSchemeHttps))
            {
                return null;
            }

            var cacheDir = new System.IO.DirectoryInfo(System.IO.Path.Combine(context.CacheDir?.AbsolutePath ?? string.Empty, "media-art"));
            if (!cacheDir.Exists)
            {
                cacheDir.Create();
            }

            var extension = System.IO.Path.GetExtension(source.AbsolutePath);
            if (string.IsNullOrWhiteSpace(extension) || extension.Length > 8)
            {
                extension = ".img";
            }

            var cacheFileName = ComputeSha256(sourceUrl) + extension;
            var localPath = System.IO.Path.Combine(cacheDir.FullName, cacheFileName);

            if (!System.IO.File.Exists(localPath)
                || new System.IO.FileInfo(localPath).Length == 0)
            {
                DownloadToFile(source, localPath);
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
