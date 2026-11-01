using System.Windows;
using System.Windows.Controls;

namespace Touchline.Wpf;

public partial class MainWindow
{
 private Border ContractDatesCard(DetailRecord detail)
 {
  var content = new StackPanel(); content.Children.Add(Text("계약 기간", 15, null, true));
  foreach (string key in new[] { "contract.starts", "contract.expires" })
  {
   var field = detail.Fields.FirstOrDefault(field => field.Key == key);
   if (field != null) content.Children.Add(new DateFieldEditor(field));
   else
   {
    var row = new Grid { Margin = new Thickness(0, 9, 0, 9) }; row.ColumnDefinitions.Add(new()); row.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
    row.Children.Add(Text(DateFieldPresentation.Label(key), 12, "#BFC9D9")); var state = Text("확인할 수 없음", 12, "#929FB3"); Grid.SetColumn(state, 1); row.Children.Add(state); content.Children.Add(row);
   }
  }
  foreach (string message in DateFieldPresentation.ContractMessages(detail)) content.Children.Add(Text(message, 11, "#929FB3"));
  return new Border { Background = ResourceBrush("PanelBrush"), BorderBrush = ResourceBrush("BorderBrush"), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(8), Padding = new Thickness(16), Margin = new Thickness(0, 0, 0, 14), Child = content };
 }
 private string? DraftDateError => _detail == null ? null : DateFieldPresentation.PeriodError(_detail.Fields);
}
