using System.ComponentModel;
using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using System.Windows.Markup;

namespace Touchline.Wpf;

/// <summary>Edits the existing scalar draft; calendar interaction never applies game changes.</summary>
internal sealed class DateFieldEditor : StackPanel
{
 private readonly EditableField _field;
 private readonly TextBlock _status;
 private bool _refreshing, _readingInput;
 internal DatePicker Picker { get; }

 public DateFieldEditor(EditableField field)
 {
  _field = field; DataContext = field; Margin = new Thickness(0, 5, 0, 9);
  var row = new Grid(); row.ColumnDefinitions.Add(new()); row.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
  var label = new TextBlock { Text = DateFieldPresentation.Label(field.Key), FontSize = 12, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(0, 0, 10, 0), TextWrapping = TextWrapping.Wrap }; label.SetResourceReference(TextBlock.ForegroundProperty, "InkBrush"); row.Children.Add(label);
  Picker = new DatePicker { DataContext = field, Width = 155, MinHeight = 32, FontSize = 12, VerticalContentAlignment = VerticalAlignment.Center, SelectedDateFormat = DatePickerFormat.Short, ToolTip = field.Hint };
  Picker.Language = XmlLanguage.GetLanguage(CultureInfo.CurrentCulture.IetfLanguageTag);
  Picker.SetResourceReference(Control.ForegroundProperty, "InkBrush"); Picker.SetResourceReference(Control.BackgroundProperty, "RaisedBrush"); Picker.SetResourceReference(Control.BorderBrushProperty, "BorderBrush");
  var dateTextStyle = new Style(typeof(DatePickerTextBox));
  dateTextStyle.Setters.Add(new Setter(Control.ForegroundProperty, Application.Current.FindResource("InkBrush")));
  dateTextStyle.Setters.Add(new Setter(Control.BackgroundProperty, Brushes.Transparent));
  dateTextStyle.Setters.Add(new Setter(Control.BorderThicknessProperty, new Thickness(0)));
  dateTextStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(6, 4, 4, 4)));
  var readOnlyStyle = new Trigger { Property = IsEnabledProperty, Value = false }; readOnlyStyle.Setters.Add(new Setter(Control.ForegroundProperty, Application.Current.FindResource("MutedBrush"))); dateTextStyle.Triggers.Add(readOnlyStyle);
  Picker.Resources[typeof(DatePickerTextBox)] = dateTextStyle;
  ToolTipService.SetShowOnDisabled(Picker, true);
  if (DateFieldPresentation.TryParse(field.Min, out var earliest)) Picker.DisplayDateStart = earliest;
  if (DateFieldPresentation.TryParse(field.Max, out var latest) && (!Picker.DisplayDateStart.HasValue || latest >= Picker.DisplayDateStart)) Picker.DisplayDateEnd = latest;
  AutomationProperties.SetName(Picker, DateFieldPresentation.Label(field.Key));
  Picker.SelectedDateChanged += (_, _) => { if (!_refreshing && field.Editable) field.EditText = Picker.SelectedDate?.ToString("yyyyMMdd", CultureInfo.InvariantCulture) ?? ""; };
  Picker.AddHandler(TextBoxBase.TextChangedEvent, new TextChangedEventHandler(InputTextChanged));
  Picker.DateValidationError += (_, args) => { args.ThrowException = false; if (!_refreshing && field.Editable) ReadInput(args.Text); };
  Grid.SetColumn(Picker, 1); row.Children.Add(Picker); Children.Add(row);
  _status = new TextBlock { FontSize = 10, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 4, 0, 0) }; Children.Add(_status);
  Loaded += (_, _) => { field.PropertyChanged -= FieldChanged; field.PropertyChanged += FieldChanged; Refresh(); };
  Unloaded += (_, _) => field.PropertyChanged -= FieldChanged;
  Refresh();
 }
 private void FieldChanged(object? sender, PropertyChangedEventArgs args)
 {
  if (args.PropertyName is nameof(EditableField.EditText) or nameof(EditableField.Editable) or nameof(EditableField.IsDirty) or nameof(EditableField.IsValid)) Refresh();
 }
 private void InputTextChanged(object sender, TextChangedEventArgs args)
 {
  if (!_refreshing && _field.Editable && args.OriginalSource is DatePickerTextBox input) ReadInput(input.Text);
 }
 private void ReadInput(string text)
 {
  _readingInput = true;
  try { _field.EditText = DateFieldPresentation.TryParseInput(text, CultureInfo.CurrentCulture, out var date) ? date.ToString("yyyyMMdd", CultureInfo.InvariantCulture) : text; }
  finally { _readingInput = false; }
 }
 private void Refresh()
 {
  _refreshing = true;
  try
  {
   Picker.IsEnabled = _field.Editable;
   bool known = DateFieldPresentation.TryParse(_field.EditText, out var date);
   bool withinRange = known && (!Picker.DisplayDateStart.HasValue || date >= Picker.DisplayDateStart.Value) && (!Picker.DisplayDateEnd.HasValue || date <= Picker.DisplayDateEnd.Value);
   if (!_readingInput)
   {
    if (withinRange) { Picker.SelectedDate = date; SyncInput(date.ToString(CultureInfo.CurrentCulture.DateTimeFormat.ShortDatePattern, CultureInfo.CurrentCulture)); }
    else if (_field.IsValid || _field.EditText.Length == 0) { Picker.SelectedDate = null; SyncInput(""); }
   }
   _status.Text = !_field.IsValid ? _field.Error : !known ? "날짜를 확인할 수 없습니다." : !withinRange ? $"현재 날짜 {DateFieldPresentation.Format(_field.EditText)} · 편집 허용 범위를 벗어났습니다." : !_field.Editable ? "읽기 전용" + (string.IsNullOrWhiteSpace(_field.Hint) ? "" : " · " + _field.Hint) : _field.IsDirty ? $"변경 전 {DateFieldPresentation.Format(_field.OriginalText)} · 미리보기 후 적용" : "달력에서 선택하거나 날짜를 입력하세요.";
   _status.SetResourceReference(TextBlock.ForegroundProperty, _field.IsValid ? "MutedBrush" : "GoldBrush");
   ToolTip = _field.Hint;
  }
  finally { _refreshing = false; }
 }
 private void SyncInput(string text)
 {
  Picker.Text = text;
  if (Picker.Template?.FindName("PART_TextBox", Picker) is DatePickerTextBox input && input.Text != text) input.Text = text;
 }
}
