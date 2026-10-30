using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Media3D;

namespace Touchline.Wpf;

/// <summary>Copy entity names without changing the current selection or FM data.</summary>
public static class NameCopy
{
 private static readonly DependencyProperty AttachedProperty = DependencyProperty.RegisterAttached(
  "Attached", typeof(bool), typeof(NameCopy), new PropertyMetadata(false));

 public static WrapPanel CreateButtons(EntityRecord entity, Action<string>? feedback = null)
  => CreateButtons(entity, feedback, Clipboard.SetText);

 /// <summary>The supplied writer allows host-side verification without touching the clipboard.</summary>
 public static WrapPanel CreateButtons(EntityRecord entity, Action<string>? feedback, Action<string> clipboardWriter)
 {
  ArgumentNullException.ThrowIfNull(entity);
  ArgumentNullException.ThrowIfNull(clipboardWriter);
  var panel = new WrapPanel { Margin = new Thickness(0, 5, 0, 0) };
  panel.Children.Add(CreateButton(entity, false, feedback, clipboardWriter));
  panel.Children.Add(CreateButton(entity, true, feedback, clipboardWriter));
  return panel;
 }

 public static void AttachGrid(DataGrid grid, Func<object?, EntityRecord?> resolve, Action<string>? feedback = null)
  => AttachGrid(grid, resolve, feedback, Clipboard.SetText);

 /// <summary>Copy commands intentionally do not select the row beneath the pointer.</summary>
 public static void AttachGrid(DataGrid grid, Func<object?, EntityRecord?> resolve, Action<string>? feedback, Action<string> clipboardWriter)
  => AttachGrid(grid, resolve, feedback, clipboardWriter, menu => menu.IsOpen = true);

 /// <summary>Inject menu presentation for offscreen verification.</summary>
 public static void AttachGrid(DataGrid grid, Func<object?, EntityRecord?> resolve, Action<string>? feedback, Action<string> clipboardWriter, Action<ContextMenu> showMenu)
  => AttachGrid(grid, resolve, feedback, clipboardWriter, showMenu, () => Keyboard.Modifiers);

 /// <summary>Inject modifier state to exercise routed keyboard events without synthetic key presses.</summary>
 public static void AttachGrid(DataGrid grid, Func<object?, EntityRecord?> resolve, Action<string>? feedback, Action<string> clipboardWriter, Action<ContextMenu> showMenu, Func<ModifierKeys> modifierKeys)
 {
  ArgumentNullException.ThrowIfNull(grid);
  ArgumentNullException.ThrowIfNull(resolve);
  ArgumentNullException.ThrowIfNull(clipboardWriter);
  ArgumentNullException.ThrowIfNull(showMenu);
  ArgumentNullException.ThrowIfNull(modifierKeys);
  if ((bool)grid.GetValue(AttachedProperty)) return;
  grid.SetValue(AttachedProperty, true);

  string? pressedIdentity = null;
  ContextMenu? openMenu = null;
  bool copying = false;

  EntityRecord? RowAt(object? source)
  {
   var row = Ancestor<DataGridRow>(source as DependencyObject);
   return row != null && ReferenceEquals(ItemsControl.ItemsControlFromItemContainer(row), grid)
    ? resolve(row.Item) : null;
  }

  async Task Copy(EntityRecord? entity, bool original)
  {
   if (copying) return;
   copying = true;
   try { await CopyAsync(entity, original, feedback, clipboardWriter); }
   finally { copying = false; }
  }

  void OpenMenu(EntityRecord entity, bool keyboard)
  {
   if (openMenu != null) openMenu.IsOpen = false;
   var menu = new ContextMenu { FontFamily = Typography.AppFont, FontSize = 13, FontWeight = FontWeights.Medium,
    PlacementTarget = grid, Placement = keyboard ? PlacementMode.Bottom : PlacementMode.MousePoint };
   menu.SetResourceReference(FrameworkElement.StyleProperty, "NameCopyMenu");
   if (keyboard && grid.ItemContainerGenerator.ContainerFromItem(grid.SelectedItem) is DataGridRow row)
    menu.PlacementTarget = row;
   foreach (bool original in new[] { false, true })
   {
    var item = new MenuItem { Header = original ? "원어 이름 복사" : "이름 복사",
     InputGestureText = original ? "Ctrl+Shift+C" : "Ctrl+C", IsEnabled = NameForCopy(entity, original) != null,
     ToolTip = NameForCopy(entity, original) ?? "원어 이름 정보가 없습니다." };
    item.SetResourceReference(FrameworkElement.StyleProperty, "NameCopyMenuItem");
    item.Click += async (_, args) => { args.Handled = true; await Copy(entity, original); };
    menu.Items.Add(item);
   }
   openMenu = menu;
   menu.Closed += (_, _) => { if (ReferenceEquals(openMenu, menu)) openMenu = null; };
   showMenu(menu);
  }

  grid.PreviewMouseRightButtonDown += (_, args) =>
  {
   pressedIdentity = null;
   if (ShouldLeaveInputAlone(args.OriginalSource as DependencyObject, grid)) return;
   pressedIdentity = RowAt(args.OriginalSource)?.Identity;
   // Handling before DataGrid sees the event prevents SelectionChanged/navigation.
   args.Handled = true;
  };
  grid.PreviewMouseRightButtonUp += (_, args) =>
  {
   if (ShouldLeaveInputAlone(args.OriginalSource as DependencyObject, grid)) { pressedIdentity = null; return; }
   args.Handled = true;
   var entity = RowAt(args.OriginalSource);
   if (entity != null && pressedIdentity == entity.Identity) OpenMenu(entity, false);
   pressedIdentity = null;
  };
  grid.PreviewKeyDown += (_, args) =>
  {
   if (HandleGridKey(grid, resolve, args.Key, modifierKeys(), args.OriginalSource as DependencyObject,
    (entity, original) => _ = Copy(entity, original), entity => OpenMenu(entity, true))) args.Handled = true;
  };
  grid.Unloaded += (_, _) => { if (openMenu != null) openMenu.IsOpen = false; pressedIdentity = null; };
 }

 private static Button CreateButton(EntityRecord entity, bool original, Action<string>? feedback, Action<string> clipboardWriter)
 {
  var button = new Button { Content = original ? "원어 이름 복사" : "이름 복사", FontFamily = Typography.AppFont,
   FontSize = 11, Padding = new Thickness(8, 5, 8, 5), Margin = new Thickness(0, 0, 6, 0),
   IsEnabled = NameForCopy(entity, original) != null,
   ToolTip = NameForCopy(entity, original) ?? "원어 이름 정보가 없습니다." };
  button.SetResourceReference(FrameworkElement.StyleProperty, "QuietButton");
  button.Click += async (_, args) =>
  {
   args.Handled = true;
   button.IsEnabled = false;
   try { await CopyAsync(entity, original, feedback, clipboardWriter); }
   finally { button.IsEnabled = NameForCopy(entity, original) != null; }
  };
  return button;
 }

 // Preserve accents, spaces and all other Unicode exactly; never transliterate or normalize.
 internal static string? NameForCopy(EntityRecord? entity, bool original)
 {
  string? name = original ? entity?.EnglishName : entity?.DisplayName;
  return string.IsNullOrWhiteSpace(name) ? null : name;
 }

 internal static async Task<bool> CopyAsync(EntityRecord? entity, bool original, Action<string>? feedback, Action<string> clipboardWriter)
 {
  string? text = NameForCopy(entity, original);
  if (text == null)
  {
   Report(original ? "원어 이름 정보가 없습니다." : "복사할 이름을 선택해 주세요.", false, feedback);
   return false;
  }
  const int attempts = 3;
  for (int attempt = 0; attempt < attempts; attempt++)
  {
   try
   {
    clipboardWriter(text);
    Report($"{(original ? "원어 이름" : "이름")} 복사 완료: {text}", false, feedback);
    return true;
   }
   catch (ExternalException) when (attempt + 1 < attempts)
   {
    // Clipboard ownership can be transient; return to the UI dispatcher between attempts.
    await Task.Delay(60);
   }
   catch (Exception error) when (error is ExternalException or System.Security.SecurityException or InvalidOperationException)
   {
    Report("이름을 복사하지 못했습니다. 다른 프로그램의 클립보드 사용이 끝난 뒤 다시 시도해 주세요.", true, feedback);
    return false;
   }
  }
  return false;
 }

 private static void Report(string message, bool error, Action<string>? feedback)
 {
  if (feedback != null) feedback(message);
  else if (error) MessageBox.Show(message, "이름 복사", MessageBoxButton.OK, MessageBoxImage.Warning);
 }

 internal static bool ShouldLeaveInputAlone(DependencyObject? source, DataGrid grid)
 {
  for (DependencyObject? current = source; current != null && !ReferenceEquals(current, grid); current = Parent(current))
  {
   if (current is TextBoxBase or PasswordBox || current is ComboBox { IsEditable: true }
    || current is DataGridCell { IsEditing: true } || current is DataGrid) return true;
  }
  return false;
 }

 internal static bool HandleGridKey(DataGrid grid, Func<object?, EntityRecord?> resolve, Key key, ModifierKeys modifiers,
  DependencyObject? source, Action<EntityRecord?, bool> copy, Action<EntityRecord> openMenu)
 {
  if (ShouldLeaveInputAlone(source, grid)) return false;
  bool copyKey = key == Key.C && (modifiers == ModifierKeys.Control || modifiers == (ModifierKeys.Control | ModifierKeys.Shift));
  bool menuKey = key == Key.Apps && modifiers == ModifierKeys.None || key == Key.F10 && modifiers == ModifierKeys.Shift;
  if (!copyKey && !menuKey) return false;
  var entity = resolve(grid.SelectedItem);
  if (copyKey) copy(entity, modifiers.HasFlag(ModifierKeys.Shift));
  else if (entity != null) openMenu(entity);
  return true;
 }

 private static T? Ancestor<T>(DependencyObject? current) where T : DependencyObject
 {
  while (current != null) { if (current is T found) return found; current = Parent(current); }
  return null;
 }

 private static DependencyObject? Parent(DependencyObject child)
 {
  if (child is Visual or Visual3D) return VisualTreeHelper.GetParent(child);
  if (child is FrameworkContentElement content) return content.Parent ?? ContentOperations.GetParent(content);
  return LogicalTreeHelper.GetParent(child);
 }
}
