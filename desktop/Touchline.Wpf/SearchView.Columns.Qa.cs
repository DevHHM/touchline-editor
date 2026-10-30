using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 public async Task<object> QaColumnsAsync(string directory)
 {
  if (_live) throw new InvalidOperationException("열 설정 QA는 오프라인 스냅샷에서만 실행합니다.");
  Directory.CreateDirectory(directory); var reports = new List<object>();
  foreach (string kind in new[] { "Player", "Staff" })
  {
   await SetKindAsync(kind);
   _suppress = true; _view.SelectedValue = "default"; _sortKey = "name"; _sort.SelectedValue = "name"; _descending = false; _suppress = false;
   await ApplyCriteriaAsync(new());
   if (!_complete || _failed != 0 || _rows.Count < 2) throw new InvalidOperationException("열 QA에는 상세가 있는 선수/스태프 2명 이상이 필요합니다.");
   if (!_grid.Columns.Any(c => c.SortMemberPath == "nationality") || !_rows.Any(r => !string.IsNullOrEmpty(r.Entity.Nationality))) throw new InvalidOperationException("기본 국적 열 또는 국적 데이터가 없습니다.");
   if (_rows.Any(r => string.IsNullOrWhiteSpace(r.Nationality))) throw new InvalidOperationException("미확인 국적 표기가 비었습니다.");
   uint[] baseline = _rows.Select(r => r.Id).Order().ToArray(); string criteria = JsonSerializer.Serialize(_criteria);
   var picker = CriteriaWindow(new()); var positionPicker = picker.QaRoleChoices();
   string scalar = _catalog.First(f => kind == "Staff" ? f.Key == "staff_youth" : f.Key == "passing").Key;
   var before = ColumnKeys();
   var dialog = new SearchColumnWindow(kind, _availableColumns, before) { Owner = Window.GetWindow(this) };
   Exception? failure = null;
   dialog.Loaded += async (_, _) => {
    try {
     dialog.QaSelect("nationality", false); dialog.QaSelect("nationality", true); dialog.QaSelect(scalar, true);
     for (int i = 0; i < 20; i++) dialog.QaMove("nationality", -1);
     if (dialog.QaKeys().Take(2).SequenceEqual(new[] {"name", "nationality"}) == false) throw new InvalidOperationException("열 순서 변경 실패.");
     if (dialog.QaFilter(scalar) < 1 || dialog.QaFilter("__no_such_column__") != 0) throw new InvalidOperationException("열 검색 실패.");
     dialog.QaFilter(""); await Dispatcher.Yield(DispatcherPriority.Render); dialog.UpdateLayout();
     CaptureColumnDialog(dialog, Path.Combine(directory, $"{kind}-column-picker.png"));
     dialog.Width=720;dialog.Height=480;await Dispatcher.Yield(DispatcherPriority.Render);dialog.UpdateLayout();
     CaptureColumnDialog(dialog, Path.Combine(directory, $"{kind}-column-picker-small.png"));
     dialog.QaAccept();
    } catch(Exception ex){failure=ex;dialog.Close();}
   };
   if (dialog.ShowDialog() != true || failure != null) throw failure ?? new InvalidOperationException("열 설정 적용 실패.");
   await ApplyColumnKeysAsync(dialog.Result!);
   if (!_complete || _failed != 0 || !baseline.SequenceEqual(_rows.Select(r=>r.Id).Order()) || criteria != JsonSerializer.Serialize(_criteria)) throw new InvalidOperationException("표시 열 변경이 검색 결과/조건을 변경했습니다.");
   if (!_loadedFields.Contains(scalar) || _rows.Any(r=>!r.Values.ContainsKey(scalar))) throw new InvalidOperationException("추가된 열을 재조회하지 않았습니다.");
   var first = _rows.First(); var detail = Read<DetailRecord>(await _bridge.CallAsync("detail",new{kind,id=first.Id}));
   if (detail.Fields.Single(f=>f.Key==scalar).Value != first.Values[scalar]) throw new InvalidOperationException("표시값이 상세와 다릅니다.");
   var saved = new SearchColumnSettings(_columnSettings.Path).Get(kind);
   if (saved.View != "custom" || !saved.Keys.SequenceEqual(dialog.Result!)) throw new InvalidOperationException("열 설정 재시작 복원 실패.");
   var cancel = new SearchColumnWindow(kind,_availableColumns,ColumnKeys()){Owner=Window.GetWindow(this)};
   cancel.Loaded += (_,_)=>{cancel.QaSelect(scalar,false);cancel.Close();};cancel.ShowDialog();
   if(cancel.Result!=null || !ColumnKeys().Contains(scalar))throw new InvalidOperationException("취소가 열을 변경했습니다.");
   _sortKey="nationality";_descending=false;ApplySort();
   var known=_resultView.Cast<SearchRow>().Where(r=>!string.IsNullOrWhiteSpace(r.Entity.Nationality)).Select(r=>r.Entity.Nationality).ToArray();
   if(!known.SequenceEqual(known.Order(StringComparer.CurrentCultureIgnoreCase)))throw new InvalidOperationException("국적 정렬 실패.");
   _descending=true;ApplySort();
   bool unknown=false;foreach(var row in _resultView.Cast<SearchRow>()){if(string.IsNullOrWhiteSpace(row.Entity.Nationality))unknown=true;else if(unknown)throw new InvalidOperationException("미확인 국적이 마지막에 오지 않습니다.");}
   // Real DataGrid drag-order callback and width round trip.
   var natColumn=_grid.Columns.Single(c=>c.SortMemberPath=="nationality");natColumn.Width=130;natColumn.DisplayIndex=2;SaveReorderedColumns();
   await Dispatcher.Yield(DispatcherPriority.Render);SaveColumnPreferences();
   saved=new SearchColumnSettings(_columnSettings.Path).Get(kind);
   if(saved.Keys[2]!="nationality" || Math.Abs(saved.Widths["nationality"]-130)>1)throw new InvalidOperationException("표 열 순서/너비 저장 실패.");

   await ApplyColumnKeysAsync(["name","nationality","club","contract.weekly_wage","contract.expires","person.dob",scalar,"ca","pa"]);
   if(!baseline.SequenceEqual(_rows.Select(r=>r.Id).Order()) || _rows.Where(r=>r.Entity.ClubId==null).Any(r=>r.Values.ContainsKey("contract.weekly_wage")))throw new InvalidOperationException("계약 없는 인물의 행/누락값 처리 실패.");
   var contract=_rows.First(r=>r.Values.ContainsKey("contract.weekly_wage"));
   var wage=(DataGridTextColumn)_grid.Columns.Single(c=>c.SortMemberPath=="contract.weekly_wage");
   var binding=(System.Windows.Data.Binding)wage.Binding;
   if(binding.Converter.Convert(contract.Values["contract.weekly_wage"],typeof(string),"contract.weekly_wage",System.Globalization.CultureInfo.CurrentCulture).ToString()!.StartsWith("£")==false)throw new InvalidOperationException("금액 단위 표시 실패.");
   if(SearchColumns.Format("contract.expires",contract.Values["contract.expires"])!="2030-06-30")throw new InvalidOperationException("날짜 표시 실패.");
   await CaptureAsync(Path.Combine(directory,$"{kind}-custom-columns.png"));
   reports.Add(new{kind,rows=_rows.Count,scalar,availableColumns=_availableColumns.Count,positionPicker,defaultNationality=true,unknownLabel=true,dialogApply=true,dialogCancel=true,requery=true,valuesMatchDetail=true,persisted=true,reorderAndWidth=true});
  }
  await SetKindAsync("Player");await ApplyCriteriaAsync(new());
  if(Selected(_view)!="custom" || !ColumnKeys().Contains("passing"))throw new InvalidOperationException("선수/스태프 보기 별도 복원 실패.");
  await CaptureAsync(Path.Combine(directory,"search-columns-ready.png"));
  return new{success=true,live=false,gameWrites=0,reports};
 }
 private static void CaptureColumnDialog(Window dialog,string path)
 {
  var content=(FrameworkElement)dialog.Content;
  if(content.ActualWidth<600 || content.ActualHeight<300)throw new InvalidOperationException("열 설정 창 크기 부족.");
  var bmp=new RenderTargetBitmap((int)Math.Ceiling(dialog.ActualWidth),(int)Math.Ceiling(dialog.ActualHeight),96,96,PixelFormats.Pbgra32);bmp.Render(dialog);
  var encoder=new PngBitmapEncoder();encoder.Frames.Add(BitmapFrame.Create(bmp));using var stream=File.Create(path);encoder.Save(stream);
 }
}
