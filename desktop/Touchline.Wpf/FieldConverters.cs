using System.Globalization;
using System.Windows;
using System.Windows.Data;
namespace Touchline.Wpf;
public sealed class BitTextConverter:IValueConverter {
 public object Convert(object value,Type type,object parameter,CultureInfo culture)=>System.Convert.ToString(value,CultureInfo.InvariantCulture)=="1";
 public object ConvertBack(object value,Type type,object parameter,CultureInfo culture)=>value is true?"1":"0";
}
public sealed class DateTextConverter:IValueConverter {
 public object? Convert(object value,Type type,object parameter,CultureInfo culture)=>DateTime.TryParseExact(System.Convert.ToString(value,CultureInfo.InvariantCulture),"yyyyMMdd",CultureInfo.InvariantCulture,DateTimeStyles.None,out var date)?date:null;
 public object ConvertBack(object value,Type type,object parameter,CultureInfo culture)=>value is DateTime date?date.ToString("yyyyMMdd",CultureInfo.InvariantCulture):Binding.DoNothing;
}
