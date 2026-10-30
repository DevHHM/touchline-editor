using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;

namespace Touchline.Wpf;

public sealed partial class SearchView
{
 public async Task<object> QaDetailedSearchAsync(string directory)
 {
  if(_live)throw new InvalidOperationException("상세 검색 QA는 스냅샷에서만 실행합니다.");
  Directory.CreateDirectory(directory);var reports=new List<object>();
  foreach(string kind in new[]{"Player","Staff"}){
   await SetKindAsync(kind);if(_sample==null)throw new InvalidOperationException("상세 검색 QA에 선수와 스태프 샘플이 필요합니다.");var sample=_sample;var field=_catalog.FirstOrDefault(f=>f.Min<=f.Value&&f.Value<=f.Max&&!f.Key.StartsWith("score_"))??throw new InvalidOperationException("상세 검색 조건 필드가 없습니다.");
   var rules=new List<SearchConditionRule>{new(){Source="entity",Key="id",Operator="eq",Value=(sample.Id==uint.MaxValue?1:sample.Id+1).ToString(CultureInfo.InvariantCulture)},new(){Join="or",Source="field",Key=field.Key,Operator="eq",Value=field.Value.ToString("G17",CultureInfo.InvariantCulture)},new(){Source="entity",Key="id",Operator="eq",Value=sample.Id.ToString(CultureInfo.InvariantCulture)},new(){Enabled=false,Source="field",Key=field.Key,Operator="gte",Value="invalid draft"}};
   var criteria=new SearchCriteria{Name=sample.Id.ToString(CultureInfo.InvariantCulture),Rules=rules};
   _view.SelectedValue="selected";var applied=await QaDialogAsync(criteria,true,directory,false);await ApplyCriteriaAsync(applied.Criteria!);
   if(!_complete||_failed!=0||_rows.Count!=1||_rows[0].Id!=sample.Id)throw new InvalidOperationException("상세 AND/OR 조건의 실제 검색 왕복 실패: "+kind);
   var serialized=JsonSerializer.Serialize(_criteria);var saved=JsonSerializer.Deserialize<SearchCriteria>(serialized,JsonDefaults.Options)!;await ApplyCriteriaAsync(saved);if(_rows.Count!=1||saved.Rules[3].Value!="invalid draft")throw new InvalidOperationException("상세 조건 저장/복원 실패");
   var none=saved.Clone();none.Rules[1].Operator="ne";await ApplyCriteriaAsync(none);if(!_complete||_failed!=0||_rows.Count!=0)throw new InvalidOperationException("상세 조건 불일치 비교 실패");await ApplyCriteriaAsync(saved);
   object? positionQuery=null;
   if(kind=="Player"){
    var positionFields=_catalog.Where(f=>PositionSearch.IsKey(f.Key)&&double.IsFinite(f.Value)).OrderByDescending(f=>f.Value).ToArray();
    var high=positionFields.FirstOrDefault(f=>f.Value>=1&&f.Value<=20);var low=positionFields.LastOrDefault(f=>high!=null&&f.Value<high.Value);
    if(high==null||low==null)throw new InvalidOperationException("복수 포지션 QA에는 서로 다른 숙련도의 위치 두 개가 필요합니다.");
    var positionCriteria=new SearchCriteria{Name=sample.Id.ToString(CultureInfo.InvariantCulture),Rules=[new(){Source=PositionSearch.Source,Key=PositionSearch.Key,Operator="gte",Value=high.Value.ToString("0",CultureInfo.InvariantCulture),Positions=[low.Key,high.Key]},new(){Source="entity",Key="id",Operator="eq",Value=sample.Id.ToString(CultureInfo.InvariantCulture)}]};
    await ApplyCriteriaAsync(positionCriteria);if(!_complete||_failed!=0||_rows.Count!=1||_rows[0].Id!=sample.Id)throw new InvalidOperationException("두 포지션 중 하나가 일치하는 실제 검색이 실패했습니다.");
    var positionRestored=JsonSerializer.Deserialize<SearchCriteria>(JsonSerializer.Serialize(_criteria),JsonDefaults.Options)!;await ApplyCriteriaAsync(positionRestored);if(_rows.Count!=1)throw new InvalidOperationException("저장한 복수 포지션 조건의 검색 복원 실패");
    positionRestored.Rules[0].Positions=[low.Key];await ApplyCriteriaAsync(positionRestored);if(!_complete||_failed!=0||_rows.Count!=0)throw new InvalidOperationException("기준 미달 포지션이 조건을 통과했습니다.");
    positionQuery=new{success=true,anyOfTwoMatches=true,singleBelowThresholdRejected=true,persistedCriteriaRequeries=true,threshold=high.Value};await ApplyCriteriaAsync(saved);
   }
   object? layout=null;var dialog=CriteriaWindow(saved);var done=new TaskCompletionSource<object?>(TaskCreationOptions.RunContinuationsAsynchronously);dialog.Loaded+=async(_,_)=>{try{layout=await dialog.QaDetailedAsync(Path.Combine(directory,kind.ToLowerInvariant()));done.TrySetResult(layout);}catch(Exception ex){done.TrySetException(ex);}finally{dialog.Close();}};dialog.ShowDialog();await done.Task;
   if(JsonSerializer.Serialize(_criteria)!=serialized)throw new InvalidOperationException("상세 조건 UI 검사가 적용된 조건을 변경했습니다.");
   await CaptureAsync(Path.Combine(directory,$"detailed-search-{kind.ToLowerInvariant()}.png"));reports.Add(new{kind,match=sample.Id,typedMixedOrAnd=true,notEqual=true,disabledDraft=true,savedRoundTrip=true,positionQuery,layout});
  }
  return new{success=true,gameWrites=false,reports};
 }
}
