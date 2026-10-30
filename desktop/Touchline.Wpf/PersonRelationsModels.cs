using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public sealed class PersonRelationTarget
{
 [JsonPropertyName("uid")] public uint Uid { get; set; }
 [JsonPropertyName("name")] public string Name { get; set; } = "";
 [JsonPropertyName("kind")] public string? Kind { get; set; }
}
public sealed class PersonNameRow
{
 [JsonPropertyName("field")] public string Field { get; set; } = "";
 [JsonPropertyName("text")] public string Text { get; set; } = "";
 public string Label => Field switch { "first" => "이름", "last" => "성", "common" => "통용명", _ => Field };
}
public sealed class PersonRelationRow
{
 [JsonPropertyName("slot")] public int Slot { get; set; }
 [JsonPropertyName("kind")] public int Kind { get; set; }
 [JsonPropertyName("kind_label")] public string KindLabel { get; set; } = "";
 [JsonPropertyName("object_type")] public int ObjectType { get; set; }
 [JsonPropertyName("reason")] public int Reason { get; set; }
 [JsonPropertyName("level")] public int Level { get; set; }
 [JsonPropertyName("target")] public PersonRelationTarget? Target { get; set; }
 [JsonPropertyName("editable_favourite")] public bool EditableFavourite { get; set; }
 [JsonPropertyName("editable_social")] public bool EditableSocial { get; set; }
 [JsonPropertyName("reason_label")] public string SocialReasonLabel { get; set; } = "";
 [JsonPropertyName("editable_secondary_nationality")] public bool EditableSecondaryNationality { get; set; }
 [JsonPropertyName("can_delete")] public bool CanDelete { get; set; }
 [JsonPropertyName("edit_reason")] public string EditReason { get; set; } = "";
 [JsonPropertyName("delete_reason")] public string DeleteReason { get; set; } = "";
 public string TargetName => Target?.Name ?? "연결 대상 없음";
 public string TargetId => Target?.Uid.ToString() ?? "—";
 public string ReasonLabel => EditableSocial ? SocialReasonLabel : EditableFavourite ? Reason switch { 0 => "없음", 1 => "서포터", 2 => "선수", 3 => "감독", 4 => "스태프", _ => "기타" } : "—";
 public string Strength => EditableSocial || EditableFavourite ? Level.ToString() : "—";
 public string Access => EditableSocial || EditableFavourite || EditableSecondaryNationality ? "편집 가능" : CanDelete ? "삭제 가능" : "게임 관리 정보";
 public string AccessReason => string.IsNullOrWhiteSpace(EditReason) && string.IsNullOrWhiteSpace(DeleteReason) ? Access : string.Join("\n", new[] { EditReason, DeleteReason }.Where(reason => !string.IsNullOrWhiteSpace(reason)));
}
public sealed class PersonCareerRow
{
 [JsonPropertyName("slot")] public int Slot { get; set; }
 [JsonPropertyName("condition")] public int Condition { get; set; }
 [JsonPropertyName("condition_label")] public string ConditionLabel { get; set; } = "";
 [JsonPropertyName("scope")] public int Scope { get; set; }
 [JsonPropertyName("scope_label")] public string ScopeLabel { get; set; } = "";
 [JsonPropertyName("job")] public int Job { get; set; }
 [JsonPropertyName("job_label")] public string JobLabel { get; set; } = "";
 [JsonPropertyName("object_type")] public int ObjectType { get; set; }
 [JsonPropertyName("object_type_label")] public string ObjectTypeLabel { get; set; } = "";
 [JsonPropertyName("target")] public PersonRelationTarget? Target { get; set; }
 [JsonPropertyName("editable_club")] public bool EditableClub { get; set; }
 [JsonPropertyName("editable_details")] public bool EditableDetails { get; set; }
 [JsonPropertyName("can_delete")] public bool CanDelete { get; set; }
 [JsonPropertyName("edit_reason")] public string EditReason { get; set; } = "";
 [JsonPropertyName("delete_reason")] public string DeleteReason { get; set; } = "";
 public string TargetName => Target?.Name ?? (ObjectType == 255 ? "대상 미설정" : $"{(string.IsNullOrWhiteSpace(ObjectTypeLabel) ? "대상" : ObjectTypeLabel)} · 이름 확인 불가");
 public string Access => EditableDetails || EditableClub ? "편집 가능" : CanDelete ? "삭제 가능" : "읽기 전용";
 public string AccessReason => string.IsNullOrWhiteSpace(EditReason) && string.IsNullOrWhiteSpace(DeleteReason) ? Access : string.Join("\n", new[] { EditReason, DeleteReason }.Where(reason => !string.IsNullOrWhiteSpace(reason)));
}
public sealed class PersonCareerData
{
 [JsonPropertyName("available")] public bool Available { get; set; }
 [JsonPropertyName("rows")] public List<PersonCareerRow> Rows { get; set; } = [];
 [JsonPropertyName("can_create")] public bool CanCreate { get; set; }
 [JsonPropertyName("can_add")] public bool CanAdd { get; set; }
 [JsonPropertyName("add_reason")] public string AddReason { get; set; } = "";
 [JsonPropertyName("notes")] public List<string> Notes { get; set; } = [];
}
public sealed class PersonRelationOption
{
 [JsonPropertyName("value")] public int Value { get; set; }
 [JsonPropertyName("label")] public string Label { get; set; } = "";
}
public sealed class PersonRelationOptions
{
 [JsonPropertyName("career_conditions")] public List<PersonRelationOption> CareerConditions { get; set; } = [];
 [JsonPropertyName("career_jobs")] public List<PersonRelationOption> CareerJobs { get; set; } = [];
}
public sealed class PersonRelationsData
{
 [JsonPropertyName("entity")] public EntityRecord Entity { get; set; } = new();
 [JsonPropertyName("names")] public List<PersonNameRow> Names { get; set; } = [];
 [JsonPropertyName("primary_nationality")] public PersonRelationTarget? PrimaryNationality { get; set; }
 [JsonPropertyName("rows")] public List<PersonRelationRow> Rows { get; set; } = [];
 [JsonPropertyName("career")] public PersonCareerData Career { get; set; } = new();
 [JsonPropertyName("spare_slots")] public int SpareSlots { get; set; }
 [JsonPropertyName("can_add_favourite")] public bool CanAddFavourite { get; set; }
 [JsonPropertyName("can_add_secondary_nationality")] public bool CanAddSecondaryNationality { get; set; }
 [JsonPropertyName("relation_add_reason")] public string RelationAddReason { get; set; } = "";
 [JsonPropertyName("favourite_add_reason")] public string FavouriteAddReason { get; set; } = "";
 [JsonPropertyName("secondary_nationality_add_reason")] public string SecondaryNationalityAddReason { get; set; } = "";
 public string FavouriteAddExplanation => string.IsNullOrWhiteSpace(FavouriteAddReason) ? RelationAddReason : FavouriteAddReason;
 public string SecondaryNationalityAddExplanation => string.IsNullOrWhiteSpace(SecondaryNationalityAddReason) ? RelationAddReason : SecondaryNationalityAddReason;
 [JsonPropertyName("notes")] public List<string> Notes { get; set; } = [];
 [JsonPropertyName("live")] public bool Live { get; set; }
 [JsonPropertyName("options")] public PersonRelationOptions Options { get; set; } = new();
}
public sealed class PersonRelationPreview
{
 [JsonPropertyName("token")] public string Token { get; set; } = "";
 [JsonPropertyName("summary")] public string Summary { get; set; } = "";
 [JsonPropertyName("before")] public string Before { get; set; } = "";
 [JsonPropertyName("after")] public string After { get; set; } = "";
 [JsonPropertyName("can_apply")] public bool CanApply { get; set; }
 [JsonPropertyName("live")] public bool Live { get; set; }
}
