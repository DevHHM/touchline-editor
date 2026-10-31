namespace Touchline.Wpf;

[Flags]
public enum ShortcutModifiers { None = 0, Control = 1, Shift = 2, Alt = 4, Windows = 8 }
public enum EditorShortcut { None, Search, SearchFilters, Refresh, Back, Forward, Review, Help }

public static class EditorShortcuts
{
 public static EditorShortcut Resolve(string key, ShortcutModifiers modifiers) => (key, modifiers) switch
 {
  ("F", ShortcutModifiers.Control) => EditorShortcut.Search,
  ("F", ShortcutModifiers.Control | ShortcutModifiers.Shift) => EditorShortcut.SearchFilters,
  ("F5", ShortcutModifiers.None) or ("R", ShortcutModifiers.Control) => EditorShortcut.Refresh,
  ("Left", ShortcutModifiers.Alt) => EditorShortcut.Back,
  ("Right", ShortcutModifiers.Alt) => EditorShortcut.Forward,
  ("S", ShortcutModifiers.Control) => EditorShortcut.Review,
  ("F1", ShortcutModifiers.None) => EditorShortcut.Help,
  _ => EditorShortcut.None
 };

 public const string HelpText = "Ctrl+F  이름·UID 검색으로 이동\nCtrl+Shift+F  검색·필터 화면\nF5 / Ctrl+R  현재 정보 새로 읽기\nAlt+← / Alt+→  이전·다음 프로필\nCtrl+S  변경 검토 열기\nF1  단축키 안내\nCtrl + / Ctrl −  화면 확대 / 축소\nCtrl+0  기본 화면 크기 (100%)\n\n변경 검토 후 적용 버튼을 눌러야 게임에 반영됩니다.\n화면 배율·창 위치·크기와 목록 너비는 다음 실행에도 유지됩니다.";
}
