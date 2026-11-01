//! Reviewed player display-name overrides. This module writes LNC files only;
//! it never changes shared game strings, game memory, or a save file.
use crate::{backend, memory::Process, model::{Entity, Session}, transaction};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs, io::Write, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

pub const MANAGED_FILE: &str = "zz_touchline_player_names_ko.lnc";
pub const MAX_PLAYERS: usize = 200;
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;
const WARNINGS: &[&str] = &[
    "한글 표시명을 LNC 파일에 설치합니다. 현재 실행 중인 FM 메모리와 저장 파일은 수정하지 않습니다.",
    "FM을 완전히 종료하고 다시 실행한 뒤 세이브를 불러와 확인하세요. 이름이 즉시 바뀌지는 않습니다.",
    "LNC는 이 FM 설치의 모든 세이브에 적용됩니다. 다른 세이브의 생성선수가 같은 UID를 사용하면 그 선수의 이름에도 영향을 줄 수 있습니다.",
    "동일 선수의 기존 이름 규칙은 표시된 파일에서 해당 줄만 정리하고 백업합니다. 그 밖의 규칙과 원본 이름·성은 유지합니다.",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parts {
    pub id: u32,
    pub name: String,
    pub english_name: String,
    pub first_name: String,
    pub last_name: String,
    pub common_name: String,
}
impl Parts {
    pub fn read(session: &Session, entity: &Entity) -> Result<Self> {
        let [first_name, last_name, common_name] = backend::player_name_parts(session, entity)?;
        let english_name = format!("{first_name} {last_name}").trim().to_owned();
        let name = if common_name.is_empty() { english_name.clone() } else { common_name.clone() };
        Ok(Self { id: entity.id, name, english_name, first_name, last_name, common_name })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    pub root: PathBuf,
    pub loaders: Vec<PathBuf>,
    /// Missing only in receipts written before FM27 support; those were FM26.
    #[serde(default)]
    pub edition: GameEdition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEdition { #[default] Fm26, Fm27 }
impl GameEdition {
    fn from_profile(profile: &str) -> Result<Self> {
        match profile.split_whitespace().next() {
            Some("FM26") => Ok(Self::Fm26),
            Some("FM27") => Ok(Self::Fm27),
            _ => bail!("현재 FM 프로필의 한글화 설치 대상을 확인하지 못했습니다"),
        }
    }
    fn database_versions(self) -> &'static [&'static str] {
        match self { Self::Fm26 => &["2600", "2620"], Self::Fm27 => &["2700"] }
    }
    fn label(self) -> &'static str { match self { Self::Fm26 => "FM26", Self::Fm27 => "FM27" } }
}
impl Installation {
    pub fn discover(session: &Session) -> Result<Self> {
        backend::validate_session(session)?;
        let process = Process::open(session.pid, false)?;
        let module = process.modules()?.into_iter().find(|m| m.name.eq_ignore_ascii_case("game_plugin.dll") && m.base == session.module_base)
            .context("현재 FM 데이터 모듈 경로를 찾지 못했습니다")?;
        let module = fs::canonicalize(&module.path).context("FM 설치 경로를 확인할 수 없습니다")?;
        let arch = module.parent().context("FM 모듈 경로 오류")?;
        let plugins = arch.parent().context("FM 플러그인 경로 오류")?;
        let data = plugins.parent().context("FM 데이터 경로 오류")?;
        ensure!(arch.file_name().is_some_and(|x| x.eq_ignore_ascii_case("x86_64"))
            && plugins.file_name().is_some_and(|x| x.eq_ignore_ascii_case("Plugins"))
            && data.file_name().is_some_and(|x| x.eq_ignore_ascii_case("fm_Data")), "검증된 FM 설치 경로가 아닙니다");
        let root = data.parent().context("FM 루트 경로 오류")?;
        ensure!(root.join("fm.exe").is_file(), "FM 실행 파일을 확인하지 못했습니다");
        let result = Self::from_root(root, GameEdition::from_profile(&session.profile)?)?;
        backend::validate_session(session)?;
        Ok(result)
    }
    fn from_root(root: &Path, edition: GameEdition) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        let mut loaders = Vec::new();
        // Older FM database directories may coexist in this installation.
        // Never install into them, or into a client-supplied destination.
        for version in edition.database_versions() {
            let loader = root.join("shared/data/database/db").join(version).join("lnc");
            if loader.join("all").is_dir() {
                checked_path(&root, &loader.join("all"), false)?;
                loaders.push(loader);
            }
        }
        ensure!(!loaders.is_empty(), "{} {}의 lnc/all 폴더가 없습니다", edition.label(), edition.database_versions().join("/"));
        Ok(Self { root, loaders, edition })
    }
    pub fn targets(&self) -> Vec<PathBuf> { self.loaders.iter().map(|p| p.join("all").join(MANAGED_FILE)).collect() }
    fn validate(&self) -> Result<()> {
        ensure!(Self::from_root(&self.root, self.edition)? == *self, "FM 설치 경로나 데이터 폴더가 변경됐습니다. 다시 검토하세요");
        Ok(())
    }
    fn validate_file(&self, path: &Path) -> Result<()> {
        ensure!(path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnc"))
            && self.loaders.iter().any(|loader| path.starts_with(loader)), "허용된 이름 파일 경로가 아닙니다");
        checked_path(&self.root, path, true)
    }
}
fn checked_path(root: &Path, path: &Path, allow_missing_leaf: bool) -> Result<()> {
    let relative = path.strip_prefix(root).context("FM 설치 폴더 밖의 경로입니다")?;
    ensure!(relative.components().all(|c| matches!(c, std::path::Component::Normal(_))), "상대 경로 이동은 허용하지 않습니다");
    let mut current = root.to_owned();
    let count = relative.components().count();
    for (i, part) in relative.components().enumerate() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                ensure!(!metadata.file_type().is_symlink(), "링크를 통한 파일 설치는 허용하지 않습니다");
                #[cfg(windows)] {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(metadata.file_attributes() & 0x400 == 0, "재분석 지점은 이름 설치 대상으로 사용할 수 없습니다");
                }
            }
            Err(e) if allow_missing_leaf && i + 1 == count && e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub type Inventory = BTreeMap<PathBuf, Vec<u8>>;
pub fn inventory(installation: &Installation) -> Result<Inventory> {
    installation.validate()?;
    fn walk(installation: &Installation, dir: &Path, depth: usize, result: &mut Inventory, total: &mut usize) -> Result<()> {
        ensure!(depth <= 8 && result.len() <= 2048, "이름 파일 폴더가 허용 범위를 넘습니다");
        checked_path(&installation.root, dir, false)?;
        for item in fs::read_dir(dir)? {
            let path = item?.path();
            checked_path(&installation.root, &path, false)?;
            if path.is_dir() { walk(installation, &path, depth + 1, result, total)?; }
            else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnc")) {
                let bytes = read_bounded(&path)?;
                *total += bytes.len();
                ensure!(*total <= MAX_TOTAL, "이름 파일 전체 크기가 허용 범위를 넘습니다");
                result.insert(path, bytes);
                ensure!(result.len() <= 2048, "이름 파일 수가 허용 범위를 넘습니다");
            }
        }
        Ok(())
    }
    let mut result = Inventory::new(); let mut total = 0;
    for loader in &installation.loaders { walk(installation, loader, 0, &mut result, &mut total)?; }
    Ok(result)
}
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    ensure!(fs::metadata(path)?.len() <= MAX_FILE, "이름 파일이 너무 큽니다: {}", path.display());
    let bytes = fs::read(path)?;
    ensure!(bytes.len() as u64 <= MAX_FILE, "조회 중 이름 파일 크기가 변경됐습니다");
    Ok(bytes)
}
fn current(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(_) => Ok(Some(read_bounded(path)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[derive(Clone, Copy, Debug)]
enum Encoding { Utf8, Utf8Bom, Utf16Le, Utf16Be }
struct TextFile { text: String, encoding: Encoding }
impl TextFile {
    fn decode(bytes: &[u8]) -> Result<Self> {
        let (encoding, body) = if bytes.starts_with(&[0xff, 0xfe]) { (Encoding::Utf16Le, &bytes[2..]) }
            else if bytes.starts_with(&[0xfe, 0xff]) { (Encoding::Utf16Be, &bytes[2..]) }
            else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) { (Encoding::Utf8Bom, &bytes[3..]) }
            else { (Encoding::Utf8, bytes) };
        let text = match encoding {
            Encoding::Utf8 | Encoding::Utf8Bom => std::str::from_utf8(body).context("지원되지 않는 LNC 인코딩입니다. UTF-8 또는 BOM이 있는 UTF-16만 지원합니다")?.to_owned(),
            Encoding::Utf16Le | Encoding::Utf16Be => {
                ensure!(body.len() % 2 == 0, "잘못된 UTF-16 이름 파일입니다");
                let words: Vec<_> = body.chunks_exact(2).map(|c| match encoding { Encoding::Utf16Le => u16::from_le_bytes([c[0], c[1]]), _ => u16::from_be_bytes([c[0], c[1]]) }).collect();
                String::from_utf16(&words).context("잘못된 UTF-16 이름 파일입니다")?
            }
        };
        ensure!(!text.contains('\0'), "NUL 문자가 포함된 이름 파일은 수정할 수 없습니다");
        Ok(Self { text, encoding })
    }
    fn encode(&self, text: &str) -> Vec<u8> {
        match self.encoding {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf8Bom => [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat(),
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let mut bytes = if matches!(self.encoding, Encoding::Utf16Le) { vec![0xff, 0xfe] } else { vec![0xfe, 0xff] };
                for word in text.encode_utf16() { bytes.extend(match self.encoding { Encoding::Utf16Le => word.to_le_bytes(), _ => word.to_be_bytes() }); }
                bytes
            }
        }
    }
}
#[derive(Clone, Debug)]
struct Rule { id: u32, first: String, common: String, last: String }
fn token<'a>(text: &mut &'a str) -> Result<Option<(&'a str, bool)>> {
    *text = text.trim_start();
    if text.is_empty() || text.starts_with('#') || text.starts_with("//") { return Ok(None); }
    if let Some(rest) = text.strip_prefix('"') {
        let end = rest.find('"').context("닫히지 않은 이름 따옴표")?;
        let value = &rest[..end];
        *text = &rest[end + 1..];
        ensure!(text.is_empty() || text.starts_with(char::is_whitespace) || text.starts_with('#') || text.starts_with("//"), "이름 토큰 뒤의 잘못된 문자");
        Ok(Some((value, true)))
    } else {
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        let value = &text[..end]; *text = &text[end..];
        Ok(Some((value, false)))
    }
}
fn parse_rule(line: &str, selected: Option<&BTreeSet<u32>>) -> Result<Option<Rule>> {
    let mut rest = line.trim_end_matches(['\r', '\n']);
    let Some((directive, _)) = token(&mut rest)? else { return Ok(None); };
    if !directive.eq_ignore_ascii_case("CHANGE_PLAYER_NAME") { return Ok(None); }
    let Some((id, _)) = token(&mut rest)? else { bail!("UID가 없는 CHANGE_PLAYER_NAME 규칙입니다"); };
    let id: u32 = id.parse().context("잘못된 CHANGE_PLAYER_NAME UID입니다")?;
    if selected.is_some_and(|ids| !ids.contains(&id)) { return Ok(None); }
    let mut values = Vec::new();
    for _ in 0..3 {
        let (value, quoted) = token(&mut rest)?.context("이름 구성 요소가 빠진 CHANGE_PLAYER_NAME 규칙입니다")?;
        ensure!(quoted && !value.chars().any(char::is_control), "이름은 따옴표로 감싸야 합니다");
        values.push(value.to_owned());
    }
    ensure!(token(&mut rest)?.is_none(), "CHANGE_PLAYER_NAME 뒤의 알 수 없는 항목입니다");
    Ok(Some(Rule { id, first: values[0].clone(), common: values[1].clone(), last: values[2].clone() }))
}
pub fn is_korean(text: &str) -> bool { text.chars().any(|c| ('가'..='힣').contains(&c)) }
fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.trim().is_empty() && name.trim() == name, "한글 이름을 입력하고 앞뒤 공백을 제거하세요");
    ensure!(name.chars().count() <= 40 && name.len() <= 120, "한글 이름은 40자·UTF-8 120바이트 이내로 입력하세요");
    ensure!(is_korean(name), "한글이 포함된 표시명을 입력하세요");
    validate_part(name)
}
fn validate_part(name: &str) -> Result<()> {
    ensure!(!name.chars().any(|c| c.is_control() || c == '"' || c == '\\'), "이름에 따옴표·역슬래시·제어문자를 사용할 수 없습니다");
    ensure!(name.len() <= 192, "원본 이름이 허용 길이를 넘습니다");
    Ok(())
}
pub fn suggestion(parts: &Parts, files: &Inventory) -> Result<(String, String, Option<String>)> {
    let ids = BTreeSet::from([parts.id]);
    let mut managed = BTreeSet::new(); let mut existing = BTreeSet::new();
    for (path, bytes) in files {
        let decoded = TextFile::decode(bytes).with_context(|| path.display().to_string())?;
        for line in decoded.text.lines() {
            if let Some(rule) = parse_rule(line, Some(&ids)).with_context(|| path.display().to_string())? {
                if rule.first == parts.first_name && rule.last == parts.last_name && is_korean(&rule.common) {
                    if path.file_name().is_some_and(|s| s.eq_ignore_ascii_case(MANAGED_FILE)) { managed.insert(rule.common); }
                    else { existing.insert(rule.common); }
                }
            }
        }
    }
    ensure!(managed.len() <= 1, "데이터 버전별 Touchline 이름 규칙이 서로 다릅니다. 설치 파일을 확인하세요");
    if let Some(name) = managed.into_iter().next() { return Ok((name.clone(), "installed".into(), Some(name))); }
    if is_korean(&parts.common_name) { return Ok((parts.common_name.clone(), "current_korean".into(), None)); }
    if existing.len() == 1 { return Ok((existing.into_iter().next().unwrap(), "existing_lnc".into(), None)); }
    Ok((String::new(), "none".into(), None))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input { pub id: u32, pub korean_name: String }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry { pub id: u32, pub name: String, pub before: String, pub after: String, pub first_name: String, pub last_name: String }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Conflict { pub id: u32, pub line: usize, pub old_name: String }
#[derive(Clone, Debug)]
pub struct FileChange { pub path: PathBuf, pub before: Option<Vec<u8>>, pub after: Option<Vec<u8>>, pub conflicts: Vec<Conflict> }
#[derive(Clone, Debug)]
pub struct Plan {
    pub session: Session,
    pub installation: Installation,
    pub parts: Vec<Parts>,
    pub entries: Vec<Entry>,
    pub files: Vec<FileChange>,
    pub inventory: Inventory,
    pub warnings: Vec<String>,
}
pub fn validate_ids(ids: &[u32]) -> Result<()> {
    ensure!(!ids.is_empty() && ids.len() <= MAX_PLAYERS, "선수는 한 번에 1–200명 선택하세요");
    let mut seen = BTreeSet::new();
    for id in ids { ensure!(*id > 0 && *id < 0x8000_0000 && seen.insert(*id), "선수 UID가 잘못됐거나 중복되었습니다"); }
    Ok(())
}
pub fn prepare(session: &Session, installation: &Installation, parts: &[Parts], inputs: &[Input]) -> Result<Plan> {
    ensure!(GameEdition::from_profile(&session.profile)? == installation.edition, "연결된 FM과 이름 파일의 게임 버전이 다릅니다. 다시 검토하세요");
    validate_ids(&inputs.iter().map(|x| x.id).collect::<Vec<_>>())?;
    ensure!(parts.len() == inputs.len(), "선수 이름 조회 자료가 일치하지 않습니다");
    let selected: BTreeSet<_> = inputs.iter().map(|x| x.id).collect();
    ensure!(parts.iter().map(|x| x.id).collect::<BTreeSet<_>>() == selected, "선수 이름 UID가 일치하지 않습니다");
    let inventory = inventory(installation)?;
    let mut entries = Vec::new();
    for input in inputs {
        validate_name(&input.korean_name)?;
        let part = parts.iter().find(|p| p.id == input.id).context("원본 이름이 없습니다")?;
        validate_part(&part.first_name)?; validate_part(&part.last_name)?;
        ensure!(!format!("{}{}", part.first_name, part.last_name).trim().is_empty(), "원본 이름과 성이 모두 비어 있습니다");
        let (_, _, installed) = suggestion(part, &inventory)?;
        entries.push(Entry { id: input.id, name: part.name.clone(), before: installed.unwrap_or_else(||part.name.clone()), after: input.korean_name.clone(), first_name: part.first_name.clone(), last_name: part.last_name.clone() });
    }
    let targets: BTreeSet<_> = installation.targets().into_iter().collect();
    for path in inventory.keys().filter(|p|p.file_name().is_some_and(|n|n.eq_ignore_ascii_case(MANAGED_FILE))) {
        ensure!(targets.contains(path), "Touchline 이름 파일의 경로나 대소문자가 예상과 다릅니다: {}. 파일을 정리한 뒤 다시 검토하세요", path.display());
    }
    let mut files = Vec::new();
    for (path, bytes) in &inventory {
        let decoded = TextFile::decode(bytes).with_context(|| path.display().to_string())?;
        let mut output = String::new(); let mut conflicts = Vec::new();
        for (i, line) in decoded.text.split_inclusive('\n').enumerate() {
            if let Some(rule) = parse_rule(line, Some(&selected)).with_context(|| format!("{}:{}", path.display(), i + 1))? {
                conflicts.push(Conflict { id: rule.id, line: i + 1, old_name: rule.common });
            } else { output.push_str(line); }
        }
        if targets.contains(path) {
            if !output.is_empty() && !output.ends_with('\n') { output.push('\n'); }
            append_entries(&mut output, &entries);
        }
        // Existing managed files retain their own encoding; new ones use UTF-8 BOM.
        let after = decoded.encode(&output);
        if after != *bytes { files.push(FileChange { path: path.clone(), before: Some(bytes.clone()), after: Some(after), conflicts }); }
    }
    for target in targets {
        if !inventory.contains_key(&target) {
            let mut output = "# Touchline player Korean display names\n# Native first/last names preserved. Applies to every save using this installation.\n".to_owned();
            append_entries(&mut output, &entries);
            let after = TextFile { text: String::new(), encoding: Encoding::Utf8Bom }.encode(&output);
            files.push(FileChange { path: target, before: None, after: Some(after), conflicts: Vec::new() });
        }
    }
    ensure!(!files.is_empty(), "선택한 이름이 이미 동일하게 설치되어 있습니다");
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Plan { session: session.clone(), installation: installation.clone(), parts: parts.to_vec(), entries, files, inventory, warnings: WARNINGS.iter().map(|s| (*s).into()).collect() })
}
fn append_entries(output: &mut String, entries: &[Entry]) {
    for e in entries { output.push_str(&format!("\"CHANGE_PLAYER_NAME\"\t{}\t\"{}\"\t\"{}\"\t\"{}\"\n", e.id, e.first_name, e.after, e.last_name)); }
}

/// Windows' SHA-256 provider is used only for file receipts, never game memory.
pub fn sha256(bytes: &[u8]) -> Result<String> {
    use std::ffi::c_void;
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptOpenAlgorithmProvider(handle: *mut *mut c_void, algorithm: *const u16, implementation: *const u16, flags: u32) -> i32;
        fn BCryptHash(handle: *mut c_void, secret: *const u8, secret_len: u32, input: *const u8, input_len: u32, output: *mut u8, output_len: u32) -> i32;
        fn BCryptCloseAlgorithmProvider(handle: *mut c_void, flags: u32) -> i32;
    }
    let algorithm: Vec<u16> = "SHA256\0".encode_utf16().collect();
    let mut handle = std::ptr::null_mut();
    ensure!(unsafe { BCryptOpenAlgorithmProvider(&mut handle, algorithm.as_ptr(), std::ptr::null(), 0) } >= 0, "SHA-256 공급자를 열 수 없습니다");
    let mut output = [0u8; 32];
    let status = unsafe { BCryptHash(handle, std::ptr::null(), 0, bytes.as_ptr(), bytes.len().try_into()?, output.as_mut_ptr(), 32) };
    unsafe { BCryptCloseAlgorithmProvider(handle, 0); }
    ensure!(status >= 0, "SHA-256 계산 실패");
    Ok(output.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn optional_hash(bytes: &Option<Vec<u8>>) -> Result<Option<String>> { bytes.as_ref().map(|b| sha256(b)).transpose() }
fn stamp() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!("{}-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
pub fn store_dir() -> PathBuf { transaction::data_dir().join("names") }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredFile {
    pub path: PathBuf,
    pub operation: String,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub backup: Option<PathBuf>,
    pub conflicts: Vec<Conflict>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    pub id: String,
    pub time: String,
    pub status: String,
    pub installation: Installation,
    pub entries: Vec<Entry>,
    pub files: Vec<StoredFile>,
    pub warnings: Vec<String>,
    pub message: String,
    pub undo_of: Option<String>,
    pub restart_required: bool,
}
fn valid_receipt_id(id: &str) -> bool { !id.is_empty() && id.len() <= 100 && id.chars().all(|c| c.is_ascii_digit() || c == '-') }
fn receipt_file(store: &Path, id: &str) -> Result<PathBuf> {
    ensure!(valid_receipt_id(id), "잘못된 이름 변경 기록 ID입니다");
    Ok(store.join(id).join("receipt.json"))
}
pub fn read_receipt(store: &Path, id: &str) -> Result<Receipt> {
    let path = receipt_file(store, id)?;
    let receipt: Receipt = serde_json::from_slice(&read_bounded(&path)?)?;
    ensure!(receipt.id == id, "이름 변경 기록 ID가 일치하지 않습니다");
    Ok(receipt)
}
pub fn history(store: &Path) -> Vec<Receipt> {
    let mut rows: Vec<Receipt> = fs::read_dir(store).into_iter().flatten().filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|id| valid_receipt_id(id))
        .filter_map(|id| read_receipt(store, &id).ok()).collect();
    rows.sort_by(|a, b| b.time.cmp(&a.time)); rows.truncate(500); rows
}
fn save_receipt(store: &Path, receipt: &Receipt) -> Result<()> { transaction::persist_json(&receipt_file(store, &receipt.id)?, receipt) }
fn lock(store: &Path) -> Result<fs::File> {
    fs::create_dir_all(store)?;
    let mut options = fs::OpenOptions::new(); options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(0); }
    options.open(store.join("write.lock")).context("다른 이름 설치 작업이 실행 중입니다")
}
fn write_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?; file.sync_all()?;
    ensure!(read_bounded(path)? == bytes, "백업 검증 실패");
    Ok(())
}
fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let temporary = path.with_file_name(format!(".touchline-name-{}-{}.tmp", std::process::id(), stamp()));
    let result = (|| {
        write_sync(&temporary, bytes)?;
        #[cfg(windows)] {
            use std::os::windows::ffi::OsStrExt;
            unsafe extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
            let from: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            ensure!(unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x8 | u32::from(replace)) } != 0, "이름 파일 교체 실패: {}", std::io::Error::last_os_error());
        }
        #[cfg(not(windows))] fs::rename(&temporary, path)?;
        ensure!(read_bounded(path)? == bytes, "설치 파일 재조회 검증 실패");
        Ok(())
    })();
    if temporary.exists() { let _ = fs::remove_file(temporary); }
    result
}
fn write_change(installation: &Installation, change: &FileChange, reverse: bool) -> Result<()> {
    installation.validate_file(&change.path)?;
    let (before, after) = if reverse { (&change.after, &change.before) } else { (&change.before, &change.after) };
    let actual = current(&change.path)?;
    if reverse && actual == *after { return Ok(()); }
    ensure!(actual == *before, "검토 이후 이름 파일이 변경됐습니다. 덮어쓰지 않았습니다: {}", change.path.display());
    match after {
        Some(bytes) => atomic_write(&change.path, bytes, before.is_some())?,
        None => fs::remove_file(&change.path)?,
    }
    ensure!(current(&change.path)? == *after, "이름 파일 검증 실패: {}", change.path.display());
    Ok(())
}
pub fn apply(plan: &Plan) -> Result<Receipt> { commit(plan, &store_dir(), None, |_, _| Ok(())) }
fn commit(plan: &Plan, store: &Path, undo_of: Option<String>, mut before_write: impl FnMut(usize, &FileChange) -> Result<()>) -> Result<Receipt> {
    ensure!(GameEdition::from_profile(&plan.session.profile)? == plan.installation.edition, "연결된 FM과 이름 파일의 게임 버전이 다릅니다. 다시 검토하세요");
    let _lock = lock(store)?;
    plan.installation.validate()?;
    ensure!(inventory(&plan.installation)? == plan.inventory, "검토 이후 LNC 파일이 추가·삭제·변경됐습니다. 다시 검토하세요");
    let id = format!("{}-{}", stamp(), std::process::id());
    let folder = store.join(&id); fs::create_dir(&folder)?;
    let mut files = Vec::new();
    for (i, change) in plan.files.iter().enumerate() {
        plan.installation.validate_file(&change.path)?;
        let backup = if let Some(bytes) = &change.before {
            let p = folder.join(format!("{i}.before.bak")); write_sync(&p, bytes)?; Some(p)
        } else { None };
        files.push(StoredFile { path: change.path.clone(), operation: if undo_of.is_some() { if change.after.is_none() { "remove" } else { "restore" } } else if change.before.is_none() { "create" } else if change.path.file_name().is_some_and(|s| s.eq_ignore_ascii_case(MANAGED_FILE)) { "update" } else { "remove_conflict" }.into(),
            before_sha256: optional_hash(&change.before)?, after_sha256: optional_hash(&change.after)?, backup, conflicts: change.conflicts.clone() });
    }
    let mut receipt = Receipt { id, time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().to_string(), status: "prepared".into(), installation: plan.installation.clone(), entries: plan.entries.clone(), files, warnings: plan.warnings.clone(), message: String::new(), undo_of, restart_required: true };
    save_receipt(store, &receipt)?;
    let mut touched = Vec::new();
    let result = (|| {
        ensure!(inventory(&plan.installation)? == plan.inventory, "백업 중 이름 파일이 변경됐습니다. 다시 검토하세요");
        for (i, change) in plan.files.iter().enumerate() {
            before_write(i, change)?;
            touched.push(i);
            write_change(&plan.installation, change, false)?;
        }
        let mut expected = plan.inventory.clone();
        for change in &plan.files {
            if let Some(bytes) = &change.after { expected.insert(change.path.clone(), bytes.clone()); }
            else { expected.remove(&change.path); }
        }
        ensure!(inventory(&plan.installation)? == expected, "설치 중 다른 이름 파일 변경이 감지됐습니다");
        receipt.status = "applied".into();
        receipt.message = if receipt.undo_of.is_some() { "LNC 파일을 이전 상태로 복원했습니다. FM 재실행 후 세이브를 다시 불러오세요." } else { "한글 이름 파일을 설치했습니다. FM 재실행 후 세이브를 다시 불러오세요." }.into();
        save_receipt(store, &receipt)?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for i in touched.into_iter().rev() {
            if let Err(e) = write_change(&plan.installation, &plan.files[i], true) { rollback_errors.push(format!("{e:#}")); }
        }
        receipt.status = if rollback_errors.is_empty() { "failed_rolled_back" } else { "failed_rollback_incomplete" }.into();
        receipt.message = format!("{error:#}");
        receipt.warnings.extend(rollback_errors);
        let persisted = save_receipt(store, &receipt);
        bail!("이름 파일 설치 실패 (기록 {} / {}): {error:#}{}", receipt.id, receipt.status, if persisted.is_err() { " · 기록 갱신 실패, 준비 단계 백업은 보존됨" } else { "" });
    }
    Ok(receipt)
}
pub fn undo(installation: &Installation, session: &Session, id: &str) -> Result<Receipt> { undo_in(installation, session, &store_dir(), id) }
fn undo_in(installation: &Installation, session: &Session, store: &Path, id: &str) -> Result<Receipt> {
    let original = read_receipt(store, id)?;
    ensure!(original.status == "applied" && original.undo_of.is_none(), "되돌릴 수 있는 이름 설치 기록이 아닙니다");
    ensure!(original.installation == *installation, "현재 연결된 FM 설치와 다른 이름 변경 기록입니다");
    ensure!(!history(store).iter().any(|r| r.status == "applied" && r.undo_of.as_deref() == Some(id)), "이미 되돌린 이름 변경입니다");
    let snapshot = inventory(installation)?;
    let mut files = Vec::new();
    for old in &original.files {
        installation.validate_file(&old.path)?;
        let before = snapshot.get(&old.path).cloned();
        ensure!(optional_hash(&before)? == old.after_sha256, "설치 이후 파일이 변경되어 되돌릴 수 없습니다: {}", old.path.display());
        let after = match &old.backup {
            Some(path) => {
                let folder = fs::canonicalize(store.join(id))?;
                let path = fs::canonicalize(path)?;
                checked_path(&folder, &path, false)?;
                ensure!(path.parent() == Some(folder.as_path()) && path.file_name().is_some_and(|n|n.to_string_lossy().ends_with(".before.bak")), "잘못된 이름 백업 경로입니다");
                let bytes = read_bounded(&path)?;
                ensure!(Some(sha256(&bytes)?) == old.before_sha256, "이름 백업 해시가 다릅니다");
                Some(bytes)
            }
            None => { ensure!(old.before_sha256.is_none(), "이름 백업 정보가 누락됐습니다"); None }
        };
        files.push(FileChange { path: old.path.clone(), before, after, conflicts: Vec::new() });
    }
    let entries = original.entries.into_iter().map(|e| Entry { before: e.after, after: e.before, ..e }).collect();
    let plan = Plan { session: session.clone(), installation: installation.clone(), parts: Vec::new(), entries, files, inventory: snapshot, warnings: WARNINGS.iter().map(|s| (*s).into()).collect() };
    commit(&plan, store, Some(id.into()), |_, _| Ok(()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub struct Fixture { pub folder: PathBuf, pub installation: Installation, pub store: PathBuf, pub session: Session, pub parts: Parts }
    impl Fixture {
        pub fn new() -> Self {
            let folder = std::env::temp_dir().join(format!("touchline-name-test-{}-{}", std::process::id(), stamp()));
            for version in ["2600", "2620", "2700", "2400"] { fs::create_dir_all(folder.join("game/shared/data/database/db").join(version).join("lnc/all")).unwrap(); }
            let installation = Installation::from_root(&folder.join("game"), GameEdition::Fm26).unwrap();
            let store = folder.join("history");
            let session = Session { pid: 0, created: 1, module_base: 1, module_size: 1, module_stamp: 1, db: 1, registry_begin: 1, game_date: "2028-07-16".into(), profile: "FM26 test-no-fm".into(), own_club_id: None };
            let parts = Parts { id: 7, name: "Viktor Bjarki Daðason".into(), english_name: "Viktor Bjarki Daðason".into(), first_name: "Viktor Bjarki".into(), last_name: "Daðason".into(), common_name: String::new() };
            Self { folder, installation, store, session, parts }
        }
        pub fn plan(&self, name: &str) -> Plan { prepare(&self.session, &self.installation, &[self.parts.clone()], &[Input { id: 7, korean_name: name.into() }]).unwrap() }
        pub fn apply(&self, plan: &Plan) -> Result<Receipt> { commit(plan,&self.store,None,|_,_|Ok(())) }
        pub fn external(&self, text: &str) -> PathBuf {
            let path = self.installation.loaders[0].join("all/previous.lnc"); fs::write(&path, text).unwrap(); path
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.folder); } }
    #[test]
    fn sha256_uses_known_standard_vector() { assert_eq!(sha256(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"); }
    #[test]
    fn original_name_parts_unicode_and_bom_are_exact_and_only_fm26_targets() {
        let f = Fixture::new(); let plan = f.plan("빅토르 다다손");
        assert_eq!(plan.files.len(), 2);
        for file in &plan.files {
            let bytes = file.after.as_ref().unwrap(); assert!(bytes.starts_with(b"\xef\xbb\xbf"));
            let text = TextFile::decode(bytes).unwrap().text;
            assert!(text.contains("\"CHANGE_PLAYER_NAME\"\t7\t\"Viktor Bjarki\"\t\"빅토르 다다손\"\t\"Daðason\"\n"));
            // Check the database path, not the random temporary root whose
            // timestamp/ID can coincidentally contain these digit sequences.
            assert!(!file.path.ends_with(Path::new("2400/lnc/all").join(MANAGED_FILE)));
            assert!(!file.path.ends_with(Path::new("2700/lnc/all").join(MANAGED_FILE)));
        }
        assert!(inventory(&f.installation).unwrap().is_empty());
    }
    #[test]
    fn fm27_targets_only_2700_and_keeps_older_database_files_unchanged() {
        let mut f=Fixture::new();
        let old_file=f.external("# FM26 must stay unchanged\n");
        let old_bytes=fs::read(&old_file).unwrap();
        f.installation=Installation::from_root(&f.folder.join("game"),GameEdition::Fm27).unwrap();
        f.session.profile="FM27 test-no-fm".into();
        let plan=f.plan("빅토르 다다손");
        assert_eq!(plan.files.len(),1);
        assert!(plan.files[0].path.ends_with(Path::new("2700/lnc/all").join(MANAGED_FILE)));
        let before=inventory(&f.installation).unwrap();
        let receipt=f.apply(&plan).unwrap();
        assert_eq!(receipt.installation.edition,GameEdition::Fm27);
        assert_eq!(fs::read(&old_file).unwrap(),old_bytes);
        undo_in(&f.installation,&f.session,&f.store,&receipt.id).unwrap();
        assert_eq!(inventory(&f.installation).unwrap(),before);
        assert_eq!(fs::read(old_file).unwrap(),old_bytes);
    }
    #[test]
    fn localization_edition_unknown_mismatch_and_missing_current_loader_fail_closed() {
        let f=Fixture::new();let mut session=f.session.clone();session.profile="FM27 test".into();
        assert!(prepare(&session,&f.installation,&[f.parts.clone()],&[Input{id:7,korean_name:"새 이름".into()}]).is_err());
        for profile in ["", "FM270 test", "unsupported FM27"] {assert!(GameEdition::from_profile(profile).is_err());}
        fs::remove_dir_all(f.folder.join("game/shared/data/database/db/2700")).unwrap();
        assert!(Installation::from_root(&f.folder.join("game"),GameEdition::Fm27).is_err(),"FM27 must not fall back to2600/2620");
    }
    #[test]
    fn pre_fm27_installation_receipts_default_to_fm26_without_losing_undo_identity() {
        let f=Fixture::new();let mut old=serde_json::to_value(&f.installation).unwrap();
        old.as_object_mut().unwrap().remove("edition");
        let decoded:Installation=serde_json::from_value(old).unwrap();
        assert_eq!(decoded,f.installation);decoded.validate().unwrap();
    }
    #[test]
    fn conflict_cleanup_preserves_unselected_crlf_comments_and_utf16_exactly() {
        let f = Fixture::new();
        let prefix = "# untouched 한글\r\n\"CHANGE_PLAYER_NAME\" 8 \"Other\" \"다른 선수\" \"Last\"\r\n";
        let selected = " \"change_player_name\"\t\"7\" \"Viktor Bjarki\" \"예전 이름\" \"Daðason\" // comment\r\n";
        for encoding in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be] {
            let decoded = TextFile { text: String::new(), encoding };
            let path = f.installation.loaders[0].join("all/previous.lnc");
            fs::write(&path, decoded.encode(&format!("{prefix}{selected}# tail"))).unwrap();
            let plan = f.plan("새 한글이름");
            let change = plan.files.iter().find(|c|c.path==path).unwrap();
            assert_eq!(change.after.as_ref().unwrap(), &decoded.encode(&format!("{prefix}# tail")));
            assert_eq!(change.conflicts.len(), 1); assert_eq!(change.conflicts[0].line, 3);
        }
    }
    #[test]
    fn malformed_selected_rule_duplicate_ids_and_injection_fail_closed() {
        let f = Fixture::new();
        for bad in ["\"CHANGE_PLAYER_NAME\" 7 \"Viktor\" \"bad", "CHANGE_PLAYER_NAME 7 first common last", "\"CHANGE_PLAYER_NAME\" 7 \"a\" \"b\" \"c\" extra"] {
            f.external(bad);
            assert!(prepare(&f.session,&f.installation,&[f.parts.clone()],&[Input{id:7,korean_name:"정상 이름".into()}]).is_err());
        }
        fs::remove_file(f.installation.loaders[0].join("all/previous.lnc")).unwrap();
        for bad in ["", "English", "한글\nINJECT", "한글\"", "한글\t", "한글\0", " 한글", "한글\\"] { assert!(validate_name(bad).is_err()); }
        assert!(validate_name(&"한".repeat(41)).is_err());
        assert!(validate_ids(&[7,7]).is_err()); assert!(validate_ids(&[0]).is_err());
        assert!(prepare(&f.session,&f.installation,&[f.parts.clone(),f.parts.clone()],&[Input{id:7,korean_name:"한글".into()},Input{id:7,korean_name:"다른 이름".into()}]).is_err());
        assert!(inventory(&f.installation).unwrap().is_empty());
    }
    #[test]
    fn suggestions_require_exact_native_names_not_only_uid() {
        let f = Fixture::new(); f.external("\"CHANGE_PLAYER_NAME\" 7 \"Wrong\" \"잘못된 이름\" \"Person\"\n");
        assert_eq!(suggestion(&f.parts,&inventory(&f.installation).unwrap()).unwrap().1,"none");
        f.external("\"CHANGE_PLAYER_NAME\" 7 \"Viktor Bjarki\" \"빅토르 다다손\" \"Daðason\"\n");
        assert_eq!(suggestion(&f.parts,&inventory(&f.installation).unwrap()).unwrap().0,"빅토르 다다손");
    }
    #[test]
    fn apply_undo_restore_precise_external_bytes_and_delete_new_managed_files() {
        let f = Fixture::new(); let original = b"# keep\r\n\"CHANGE_PLAYER_NAME\" 7 \"Viktor Bjarki\" \"Old\" \"Dada\"\r\n# last";
        let path = f.external(std::str::from_utf8(original).unwrap());
        let before = inventory(&f.installation).unwrap(); let plan = f.plan("빅토르 다다손");
        let receipt = commit(&plan,&f.store,None,|_,_|Ok(())).unwrap();
        assert_eq!(receipt.status,"applied"); assert_eq!(fs::read(&path).unwrap(),b"# keep\r\n# last");
        assert!(receipt.files.iter().filter_map(|x|x.backup.as_ref()).all(|p|p.is_file()));
        let undo = undo_in(&f.installation,&f.session,&f.store,&receipt.id).unwrap();
        assert_eq!(undo.undo_of.as_deref(),Some(receipt.id.as_str()));
        assert_eq!(inventory(&f.installation).unwrap(),before);
        assert!(undo_in(&f.installation,&f.session,&f.store,&receipt.id).is_err());
    }
    #[test]
    fn changed_or_new_external_file_rejects_before_any_install() {
        let f = Fixture::new(); let plan = f.plan("빅토르 다다손"); f.external("# concurrent\n");
        assert!(commit(&plan,&f.store,None,|_,_|Ok(())).is_err());
        assert!(f.installation.targets().iter().all(|p|!p.exists()));
        assert!(history(&f.store).is_empty());
    }
    #[test]
    fn rollback_restores_every_touched_file_after_midway_failure() {
        let f = Fixture::new(); f.external("\"CHANGE_PLAYER_NAME\" 7 \"A\" \"B\" \"C\"\n");
        let before = inventory(&f.installation).unwrap(); let plan = f.plan("빅토르 다다손");
        assert!(commit(&plan,&f.store,None,|i,_|if i==1 {bail!("simulated disk failure")}else{Ok(())}).is_err());
        assert_eq!(inventory(&f.installation).unwrap(),before);
        assert_eq!(history(&f.store)[0].status,"failed_rolled_back");
    }
    #[test]
    fn rollback_and_undo_do_not_overwrite_concurrent_changes() {
        let f = Fixture::new(); let plan = f.plan("빅토르 다다손");
        let first = plan.files[0].path.clone();
        assert!(commit(&plan,&f.store,None,|i,_|if i==1 {fs::write(&first,b"# external edit")?;bail!("fail")}else{Ok(())}).is_err());
        assert_eq!(fs::read(&first).unwrap(),b"# external edit");
        assert_eq!(history(&f.store)[0].status,"failed_rollback_incomplete");
        let plan = f.plan("빅토르 다다손"); let receipt = commit(&plan,&f.store,None,|_,_|Ok(())).unwrap();
        fs::write(&first,b"# another external edit").unwrap();
        assert!(undo_in(&f.installation,&f.session,&f.store,&receipt.id).is_err());
        assert_eq!(fs::read(&first).unwrap(),b"# another external edit");
    }
    #[test]
    fn managed_file_retains_unselected_names_and_repeated_same_preview_is_noop() {
        let f = Fixture::new();
        let plan = f.plan("빅토르 다다손"); commit(&plan,&f.store,None,|_,_|Ok(())).unwrap();
        assert!(prepare(&f.session,&f.installation,&[f.parts.clone()],&[Input{id:7,korean_name:"빅토르 다다손".into()}]).unwrap_err().to_string().contains("이미 동일"));
        let mut other = f.parts.clone(); other.id=8;
        let plan=prepare(&f.session,&f.installation,&[other],&[Input{id:8,korean_name:"다른 선수".into()}]).unwrap();
        commit(&plan,&f.store,None,|_,_|Ok(())).unwrap();
        for path in f.installation.targets() { let text=TextFile::decode(&fs::read(path).unwrap()).unwrap().text; assert!(text.contains("\t7\t"));assert!(text.contains("\t8\t")); }
    }
    #[test]
    fn before_restart_second_install_uses_installed_alias_as_preview_before() {
        let f = Fixture::new(); let first = f.plan("첫 번째 이름"); commit(&first,&f.store,None,|_,_|Ok(())).unwrap();
        let second = f.plan("두 번째 이름");
        assert_eq!(second.entries[0].before,"첫 번째 이름");
        assert_eq!(second.entries[0].name,f.parts.name);
        let receipt=commit(&second,&f.store,None,|_,_|Ok(())).unwrap();
        let undo=undo_in(&f.installation,&f.session,&f.store,&receipt.id).unwrap();
        assert_eq!(undo.entries[0].after,"첫 번째 이름");
    }
    #[test]
    fn differently_cased_managed_filename_rejected_before_writes() {
        let f = Fixture::new();
        fs::write(f.installation.loaders[0].join("all").join(MANAGED_FILE.to_uppercase()),b"# keep").unwrap();
        assert!(prepare(&f.session,&f.installation,&[f.parts.clone()],&[Input{id:7,korean_name:"한글 이름".into()}]).is_err());
        assert!(history(&f.store).is_empty());
    }
    #[test]
    fn unsafe_paths_and_receipt_ids_are_rejected() {
        let f=Fixture::new();
        assert!(f.installation.validate_file(&f.folder.join("outside.lnc")).is_err());
        let traversal=PathBuf::from(format!("{}\\all\\..\\outside.lnc",f.installation.loaders[0].display()));
        assert!(f.installation.validate_file(&traversal).is_err());
        assert!(read_receipt(&f.store,"../other").is_err());
        assert!(read_receipt(&f.store,"..\\other").is_err());
    }
}
