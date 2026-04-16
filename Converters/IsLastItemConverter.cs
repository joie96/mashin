using mashin.Models;
using System.Diagnostics;
using System.Globalization;

namespace mashin.Converters;

/// <summary>
/// Determines whether the current <see cref="Artist"/> item is the last element in the provided list.
/// </summary>
public class IsLastItemConverter : IMultiValueConverter
{
    public object Convert(object[] values, Type targetType, object parameter, CultureInfo culture)
    {

        if (values == null || values.Length != 2)
        {
            return false;
        }


        if (values[0] is Artist artist && values[1] is IList<Artist> artists)
        {
            var index = artists.IndexOf(artist);
            var isLast = index == artists.Count - 1;
            return isLast;
        }

        return false;
    }

    public object[] ConvertBack(object value, Type[] targetTypes, object parameter, CultureInfo culture)
    {
        throw new NotImplementedException();
    }
}