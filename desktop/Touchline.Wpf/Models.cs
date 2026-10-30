using System.ComponentModel;
using System.Globalization;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Touchline.Wpf;

public static class JsonDefaults
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNameCaseInsensitive = true,
        NumberHandling = JsonNumberHandling.Strict
    };
}

public sealed class EntityRecord : INotifyPropertyChanged
{
    [JsonPropertyName("id")] public uint Id { get; set; }
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("english_name")] public string EnglishName { get; set; } = "";
    [JsonPropertyName("nationality")] public string Nationality { get; set; } = "";
    [JsonPropertyName("nationality_id")] public uint? NationalityId { get; set; }
    [JsonIgnore] public string NationalityLabel => string.IsNullOrWhiteSpace(Nationality) ? "미확인" : Nationality;
    [JsonPropertyName("club")] public string Club { get; set; } = "";
    [JsonPropertyName("club_id")] public uint? ClubId { get; set; }
    [JsonPropertyName("age")] public uint? Age { get; set; }
    [JsonPropertyName("ca")] public ushort? Ca { get; set; }
    [JsonPropertyName("pa")] public ushort? Pa { get; set; }
    [JsonPropertyName("role")] public string Role { get; set; } = "";
    [JsonPropertyName("group")] public string Group { get; set; } = "";
    [JsonPropertyName("football_category")] public string FootballCategory { get; set; } = "unknown";
    [JsonIgnore] public string DisplayName => string.IsNullOrWhiteSpace(Name) ? EnglishName : Name;
    [JsonIgnore] public string KindLabel => Kind switch
    {
        "Player" => "선수", "Staff" => "스태프", "Club" => "구단",
        "Person" => "기타 인물", "City" => "도시", _ => Kind
    };
    [JsonIgnore] public string AbilityText => Ca.HasValue || Pa.HasValue ? $"{Ca?.ToString() ?? "—"} / {Pa?.ToString() ?? "—"}" : "—";
    [JsonIgnore] public string AgeText => Age.HasValue ? $"{Age}세" : "—";
    [JsonIgnore] public string FootballCategoryLabel => FootballCategory switch { "men" => "남자축구", "women" => "여자축구", _ => "미확인" };
    [JsonIgnore] public int PositionRank => PositionOrder.Rank(Role);
    [JsonIgnore] public string Identity => $"{Kind}:{Id}";
    private ClubGrowthItem? _growth;
    private string _growthMessage = "성장 정보를 불러오는 중입니다.";
    [JsonIgnore] public ClubGrowthItem? Growth => _growth;
    [JsonIgnore] public long? RosterAge => _growth == null ? Age : _growth.CurrentAge;
    [JsonIgnore] public long? RosterCa => _growth == null ? Ca : _growth.CurrentCa;
    [JsonIgnore] public long? RosterPa => _growth == null ? Pa : _growth.PotentialCa;
    [JsonIgnore] public long? GrowthReference => _growth?.Status == "calculated" ? _growth.ReferenceValue : null;
    [JsonIgnore] public long? GrowthGap => GrowthReference.HasValue ? _growth?.ReferenceGap : null;
    [JsonIgnore] public string GrowthReferenceText => GrowthReference?.ToString("N0", CultureInfo.CurrentCulture) ?? "미확인";
    [JsonIgnore] public string GrowthGapText => GrowthGap?.ToString("+0;-0;0", CultureInfo.CurrentCulture) ?? "—";
    [JsonIgnore] public string GrowthDetails => _growth?.Describe() ?? _growthMessage;
    public void SetGrowth(ClubGrowthItem? item, string message = "응답에 이 선수의 성장 정보가 없습니다.")
    {
        _growth = item; _growthMessage = message;
        foreach (string property in new[] { nameof(Growth), nameof(RosterAge), nameof(RosterCa), nameof(RosterPa), nameof(GrowthReference), nameof(GrowthGap), nameof(GrowthReferenceText), nameof(GrowthGapText), nameof(GrowthDetails) })
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(property));
    }
    public event PropertyChangedEventHandler? PropertyChanged;
}

public sealed class ClubGrowthResponse
{
    [JsonPropertyName("club_id")] public uint ClubId { get; set; }
    [JsonPropertyName("formula_version")] public string FormulaVersion { get; set; } = "";
    [JsonPropertyName("scope")] public string Scope { get; set; } = "";
    [JsonPropertyName("game_date")] public string GameDate { get; set; } = "";
    [JsonPropertyName("items")] public List<ClubGrowthItem> Items { get; set; } = [];
    [JsonPropertyName("warnings")] public List<string> Warnings { get; set; } = [];
}

public sealed class ClubGrowthItem
{
    public const string Disclaimer = "최종 CA 예측·보장 아님 / 후속 보정 미포함";
    [JsonPropertyName("player_id")] public uint PlayerId { get; set; }
    [JsonPropertyName("status")] public string Status { get; set; } = "unknown";
    [JsonPropertyName("reference_value")] public long? ReferenceValue { get; set; }
    [JsonPropertyName("base_reference_value")] public long? BaseReferenceValue { get; set; }
    [JsonPropertyName("facility_ceiling")] public long? FacilityCeiling { get; set; }
    [JsonPropertyName("native_facility_value")] public long? NativeFacilityValue { get; set; }
    [JsonPropertyName("current_ca")] public long? CurrentCa { get; set; }
    [JsonPropertyName("potential_ca")] public long? PotentialCa { get; set; }
    [JsonPropertyName("current_age")] public long? CurrentAge { get; set; }
    [JsonPropertyName("person_index")] public long? PersonIndex { get; set; }
    [JsonPropertyName("reference_gap")] public long? ReferenceGap { get; set; }
    [JsonPropertyName("reason")] public string Reason { get; set; } = "";
    [JsonPropertyName("selected_team")] public string? SelectedTeam { get; set; }
    [JsonPropertyName("actual_team")] public string? ActualTeam { get; set; }
    [JsonPropertyName("source_team_kind")] public string? SourceTeamKind { get; set; }
    [JsonPropertyName("team_reputation")] public long? TeamReputation { get; set; }
    [JsonPropertyName("competition_reputation")] public long? CompetitionReputation { get; set; }
    [JsonPropertyName("denominator")] public long? Denominator { get; set; }
    [JsonPropertyName("age_reference")] public long? AgeReference { get; set; }
    [JsonPropertyName("calculation_note")] public string CalculationNote { get; set; } = "";

    public string Describe()
    {
        static string Number(long? value) => value?.ToString("N0", CultureInfo.CurrentCulture) ?? "미확인";
        string status = Status == "calculated" && ReferenceValue.HasValue ? $"성장 참고치 {Number(ReferenceValue)} · CA 대비 {ReferenceGap?.ToString("+0;-0;0") ?? "—"}" : "미확인 · " + (string.IsNullOrWhiteSpace(Reason) ? "계산에 필요한 정보를 확인하지 못했습니다." : Reason);
        string team = $"실제 소속 {ActualTeam ?? "미확인"} · 적용 팀 {SelectedTeam ?? "미확인"}" + (string.IsNullOrWhiteSpace(SourceTeamKind) ? "" : $" ({SourceTeamKind})");
        string inputs = $"팀 명성 {Number(TeamReputation)} · 대회 명성 {Number(CompetitionReputation)} · 분모 {Number(Denominator)} · 연령·PA 참조값 {Number(AgeReference)}";
        string current = $"조회 시점 나이 {Number(CurrentAge)} · CA {Number(CurrentCa)} · PA {Number(PotentialCa)}";
        string facility = FacilityCeiling.HasValue ? $"\n기본 참고치 {Number(BaseReferenceValue)} · 시설 관련 보정 상한 {Number(FacilityCeiling)} (시설 값 {Number(NativeFacilityValue)})" : "";
        string potential = CurrentCa.HasValue && PotentialCa.HasValue && CurrentCa >= PotentialCa
            ? "\n현재 CA는 PA에 도달했습니다. 참고치 차이는 추가 성장 여지가 아닙니다."
            : ReferenceValue.HasValue && PotentialCa.HasValue && ReferenceValue > PotentialCa
                ? "\n참고치가 PA보다 높아도 실제 성장의 잠재력 한계는 PA입니다." : "";
        return $"{status}\n{team}\n{inputs}\n{current}{facility}{potential}\n{CalculationNote}\nCA 대비 = 참고치 − 현재 CA · 음수는 쇠퇴 예측이 아닙니다.\n{Disclaimer}";
    }
}

public sealed class DetailRecord
{
    [JsonPropertyName("entity")] public EntityRecord Entity { get; set; } = new();
    [JsonPropertyName("fields")] public List<EditableField> Fields { get; set; } = [];
    [JsonPropertyName("notes")] public List<string> Notes { get; set; } = [];
    [JsonPropertyName("scores")] public JsonElement Scores { get; set; }
    [JsonPropertyName("image_path")] public string? ImagePath { get; set; }
    [JsonPropertyName("badge_path")] public string? BadgePath { get; set; }
    [JsonPropertyName("city_id")] public uint? CityId { get; set; }

    /// <summary>Call once after loading, never during an edit refresh.</summary>
    public void InitializeFields()
    {
        foreach (var field in Fields) field.SetBaseline(field.Value);
    }
}

/// <summary>
/// The numeric baseline remains exact even when its displayed text is rounded.
/// WPF edits bind to EditText. Value's setter is for deserialization/baseline loading.
/// Step controls spin-button increments, not a permitted-value lattice.
/// </summary>
public sealed class EditableField : INotifyPropertyChanged, IDataErrorInfo
{
    private double _original;
    private double _value;
    private string _editText = "";
    private string _baselineText = "";
    private string _error = "";
    private bool _editable;
    private double _minimum;
    private double _maximum;
    private double _step = 1;
    private bool _showPrecision;

    [JsonPropertyName("key")] public string Key { get; set; } = "";
    [JsonPropertyName("label")] public string Label { get; set; } = "";
    [JsonPropertyName("section")] public string Section { get; set; } = "";
    [JsonPropertyName("hint")] public string Hint { get; set; } = "";
    [JsonPropertyName("min")] public double Min { get => _minimum; set { _minimum = value; OnPropertyChanged(); } }
    [JsonPropertyName("max")] public double Max { get => _maximum; set { _maximum = value; OnPropertyChanged(); } }
    [JsonPropertyName("step")] public double Step { get => _step; set { _step = value; OnPropertyChanged(); RefreshBaselineText(); } }
    [JsonPropertyName("editable")] public bool Editable { get => _editable; set { _editable = value; OnPropertyChanged(); } }
    [JsonPropertyName("value")] public double Value { get => _value; set => SetBaseline(value); }
    [JsonIgnore] public double Original => _original;
    [JsonIgnore] public bool IsDirty => !IsValid ? _editText != _baselineText : !_value.Equals(_original);
    [JsonIgnore] public bool IsValid => _error.Length == 0;
    [JsonIgnore] public string Error => _error;
    [JsonIgnore] public string OriginalText => Format(_original);
    [JsonIgnore] public bool ShowPrecision
    {
        get => _showPrecision;
        set
        {
            if (_showPrecision == value) return;
            _showPrecision = value;
            RefreshBaselineText();
            OnPropertyChanged();
        }
    }
    [JsonIgnore] public string ColorHex => _value >= 16 && Max <= 20 ? "#5DD6AD" : _value >= 11 && Max <= 20 ? "#EEC46E" : "#C6D0DC";
    [JsonIgnore] public string EditText
    {
        get => _editText;
        set
        {
            value ??= "";
            if (_editText == value) return;
            if (!Editable) return;
            _editText = value;
            if (!double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed) || !double.IsFinite(parsed))
                _error = "숫자를 입력하세요. 소수점은 . 을 사용합니다.";
            else if (parsed < Min || parsed > Max)
                _error = $"{Min.ToString("G", CultureInfo.InvariantCulture)} ~ {Max.ToString("G", CultureInfo.InvariantCulture)} 범위로 입력하세요.";
            else
            {
                _value = parsed;
                _error = "";
            }
            NotifyValueState();
        }
    }

    string IDataErrorInfo.this[string columnName] => columnName == nameof(EditText) ? Error : "";

    public void SetBaseline(double value)
    {
        _original = _value = value;
        _error = double.IsFinite(value) ? "" : "유효하지 않은 원본 값입니다.";
        _baselineText = _editText = Format(value);
        NotifyValueState();
        OnPropertyChanged(nameof(Original));
        OnPropertyChanged(nameof(OriginalText));
    }

    public void Reset() => SetBaseline(_original);

    public void Adjust(double steps)
    {
        if (!Editable || !double.IsFinite(steps) || !double.IsFinite(Step) || Step <= 0 || Min > Max) return;
        var value = Math.Clamp((IsValid ? _value : _original) + Step * steps, Min, Max);
        EditText = value.ToString("G17", CultureInfo.InvariantCulture);
    }

    private string Format(double value) => value.ToString(
        ShowPrecision ? "0.########" : (Max <= 20 && Min >= 0 || Step >= 1 ? "0" : "0.########"),
        CultureInfo.InvariantCulture);

    private void RefreshBaselineText()
    {
        if (IsDirty) return;
        _baselineText = _editText = Format(_original);
        OnPropertyChanged(nameof(EditText));
        OnPropertyChanged(nameof(OriginalText));
    }

    private void NotifyValueState()
    {
        OnPropertyChanged(nameof(Value));
        OnPropertyChanged(nameof(EditText));
        OnPropertyChanged(nameof(IsDirty));
        OnPropertyChanged(nameof(IsValid));
        OnPropertyChanged(nameof(Error));
        OnPropertyChanged(nameof(ColorHex));
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? propertyName = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
}
