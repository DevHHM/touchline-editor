//! Local FM graphics support. Config indexing and decoding never run in a UI frame.
//! This reads installed graphics only; it neither changes FM files nor downloads art.
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
    time::Duration,
};

use crate::model::EntityKind;
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

const TEXTURE_LIMIT: usize = 128;
const MAX_CONFIGS: usize = 4_096;
const MAX_DIRECTORIES: usize = 16_384;
const MAX_MAPPINGS: usize = 1_500_000;
const MAX_CONFIG_BYTES: u64 = 96 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ImageKind {
    Portrait,
    Badge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ImageKey {
    kind: ImageKind,
    id: u32,
}

impl ImageKey {
    fn from_entity(kind: EntityKind, id: u32) -> Option<Self> {
        Some(Self {
            kind: match kind {
                EntityKind::Player | EntityKind::Staff | EntityKind::Person => ImageKind::Portrait,
                EntityKind::Club => ImageKind::Badge,
                EntityKind::City => return None,
            },
            id,
        })
    }
}

#[derive(Clone, Debug)]
struct Mapping {
    stem: PathBuf,
    priority: (usize, u8, String),
}

enum Command {
    Load(ImageKey),
}
enum Reply {
    Indexed {
        mappings: usize,
        configs: usize,
        missing_roots: usize,
    },
    Loaded(ImageKey, Option<ColorImage>),
}

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    roots: Vec<PathBuf>,
}

/// A bounded, asynchronous portrait/badge cache for native UI.
pub struct ImageAssets {
    roots: Vec<PathBuf>,
    tx: SyncSender<Command>,
    rx: Receiver<Reply>,
    textures: HashMap<ImageKey, TextureHandle>,
    recent: VecDeque<ImageKey>,
    requested: HashSet<ImageKey>,
    missing: HashSet<ImageKey>,
    indexed: bool,
    status: String,
}

impl Default for ImageAssets {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageAssets {
    pub fn new() -> Self {
        let configured = settings_path()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .map(|settings| settings.roots)
            .unwrap_or_default();
        let roots = if configured.is_empty() {
            default_roots()
        } else {
            configured
        };
        Self::with_roots(roots)
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        let (tx, rx) = start_worker(roots.clone());
        Self {
            roots,
            tx,
            rx,
            textures: HashMap::new(),
            recent: VecDeque::new(),
            requested: HashSet::new(),
            missing: HashSet::new(),
            indexed: false,
            status: "사진·구단 로고를 찾고 있습니다…".into(),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
    pub fn status(&self) -> String {
        self.status.clone()
    }
    pub fn assets_ready(&self) -> bool {
        self.indexed
    }
    pub fn is_loading(&self) -> bool {
        !self.indexed || !self.requested.is_empty()
    }

    /// Earlier roots have priority. Persists only this application's preferences.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        let roots = deduplicate(roots);
        if let Some(path) = settings_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(bytes) = serde_json::to_vec_pretty(&Settings {
                roots: roots.clone(),
            }) {
                let _ = fs::write(path, bytes);
            }
        }
        *self = Self::with_roots(roots);
    }

    pub fn refresh(&mut self) {
        *self = Self::with_roots(self.roots.clone());
    }

    /// Drain results and schedule a repaint only while background work is pending.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Indexed {
                    mappings,
                    configs,
                    missing_roots,
                } => {
                    self.indexed = true;
                    self.status = if mappings == 0 {
                        "사진 폴더를 찾지 못했습니다. FM graphics 경로를 지정해 주세요.".into()
                    } else {
                        format!(
                            "사진·로고 {mappings}개 · 설정 {configs}개{}",
                            if missing_roots > 0 {
                                " · 일부 경로 없음"
                            } else {
                                ""
                            }
                        )
                    };
                }
                Reply::Loaded(key, decoded) => {
                    self.requested.remove(&key);
                    if let Some(decoded) = decoded {
                        let texture = ctx.load_texture(
                            format!("fm-{:?}-{}", key.kind, key.id),
                            decoded,
                            TextureOptions::LINEAR,
                        );
                        self.textures.insert(key, texture);
                        self.touch(key);
                        self.trim();
                    } else {
                        // Bound negative cache as well as decoded GPU resources.
                        if self.missing.len() >= 16_384 {
                            self.missing.clear();
                        }
                        self.missing.insert(key);
                    }
                }
            }
        }
        if self.is_loading() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    pub fn image(
        &mut self,
        ctx: &egui::Context,
        kind: EntityKind,
        id: u32,
    ) -> Option<TextureHandle> {
        self.poll(ctx);
        let key = ImageKey::from_entity(kind, id)?;
        if let Some(texture) = self.textures.get(&key).cloned() {
            self.touch(key);
            return Some(texture);
        }
        if !self.missing.contains(&key) && !self.requested.contains(&key) {
            match self.tx.try_send(Command::Load(key)) {
                Ok(()) => {
                    self.requested.insert(key);
                }
                Err(TrySendError::Full(_)) => {
                    ctx.request_repaint_after(Duration::from_millis(80));
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.missing.insert(key);
                }
            }
        }
        None
    }

    fn touch(&mut self, key: ImageKey) {
        self.recent.retain(|old| *old != key);
        self.recent.push_back(key);
    }
    fn trim(&mut self) {
        while self.textures.len() > TEXTURE_LIMIT {
            if let Some(key) = self.recent.pop_front() {
                self.textures.remove(&key);
            } else {
                break;
            }
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| {
        PathBuf::from(p)
            .join("TouchlineEditor")
            .join("graphics.json")
    })
}

pub fn default_roots() -> Vec<PathBuf> {
    roots_for_edition(None)
}

/// A connected game's conventional folders must not fall back to another
/// edition: generated player UIDs can identify different people there.
pub fn default_roots_for_profile(profile: &str) -> Vec<PathBuf> {
    roots_for_edition(Some(profile))
}

fn conventional_roots(homes: impl IntoIterator<Item = PathBuf>, profile: Option<&str>) -> Vec<PathBuf> {
    let editions: &[&str] = match profile.map(|p| p.split_whitespace().next().unwrap_or("")) {
        Some("FM27") => &["Football Manager 27", "Football Manager 2027"],
        Some("FM26") => &["Football Manager 26", "Football Manager 2026"],
        Some(_) => &[],
        None => &["Football Manager 27", "Football Manager 2027", "Football Manager 26", "Football Manager 2026"],
    };
    homes.into_iter().flat_map(|home| editions.iter().map(move |edition| {
        home.join("Documents").join("Sports Interactive").join(edition).join("graphics")
    })).collect()
}

fn roots_for_edition(profile: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(custom) = std::env::var_os("TOUCHLINE_GRAPHICS_ROOTS") {
        paths.extend(std::env::split_paths(&custom));
    }
    if let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_owned))
    {
        let bundled = exe.join("graphics");
        if bundled.is_dir() {
            paths.push(bundled);
        }
    }
    let conventional = conventional_roots([
        std::env::var_os("USERPROFILE"),
        std::env::var_os("OneDrive"),
    ]
    .into_iter()
    .flatten()
    .map(PathBuf::from), profile);
    paths.extend(conventional.iter().filter(|path| path.is_dir()).cloned());
    if paths.is_empty() {
        paths.extend(conventional.into_iter().take(1));
    }
    deduplicate(paths)
}

fn deduplicate(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.to_string_lossy().to_lowercase()))
        .collect()
}

fn start_worker(roots: Vec<PathBuf>) -> (SyncSender<Command>, Receiver<Reply>) {
    let (tx, commands) = mpsc::sync_channel(256);
    // Keep pending decoded pixel buffers small while the window is minimized.
    let (replies, rx) = mpsc::sync_channel(8);
    thread::Builder::new()
        .name("fm-local-graphics".into())
        .spawn(move || {
            let (index, configs, missing_roots) = build_index(&roots);
            if replies
                .send(Reply::Indexed {
                    mappings: index.len(),
                    configs,
                    missing_roots,
                })
                .is_err()
            {
                return;
            }
            while let Ok(Command::Load(key)) = commands.recv() {
                let decoded = index.get(&key).and_then(|m| decode_image(&m.stem));
                if replies.send(Reply::Loaded(key, decoded)).is_err() {
                    break;
                }
            }
        })
        .expect("local graphics worker");
    (tx, rx)
}

fn build_index(roots: &[PathBuf]) -> (HashMap<ImageKey, Mapping>, usize, usize) {
    let mut maps: HashMap<ImageKey, Mapping> = HashMap::new();
    let mut configs_count = 0;
    let mut missing_roots = 0;
    for (root_priority, root) in roots.iter().enumerate() {
        if !root.is_dir() {
            missing_roots += 1;
            continue;
        }
        let mut configs = Vec::new();
        let mut todo = vec![root.clone()];
        let mut directories = 0;
        while let Some(dir) = todo.pop() {
            directories += 1;
            if directories > MAX_DIRECTORIES || configs.len() >= MAX_CONFIGS {
                break;
            }
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if kind.is_dir()
                    && !name.starts_with('.')
                    && !name.contains("backup")
                    && name != "cache"
                {
                    todo.push(entry.path());
                } else if kind.is_file() && name == "config.xml" {
                    configs.push(entry.path());
                }
            }
        }
        configs.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        for path in configs {
            if maps.len() >= MAX_MAPPINGS {
                break;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.len() > MAX_CONFIG_BYTES {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let xml = String::from_utf8_lossy(&bytes);
            configs_count += 1;
            for (key, stem, quality) in parse_config(&path, &xml) {
                if maps.len() >= MAX_MAPPINGS {
                    break;
                }
                let priority = (
                    root_priority,
                    quality,
                    path.to_string_lossy().to_lowercase(),
                );
                let candidate = Mapping { stem, priority };
                match maps.get(&key) {
                    Some(old) if old.priority <= candidate.priority => {}
                    _ => {
                        maps.insert(key, candidate);
                    }
                }
            }
        }
    }
    (maps, configs_count, missing_roots)
}

/// Parse FM's simple record mappings; never evaluates entities, DTDs or includes.
fn parse_config(config: &Path, xml: &str) -> Vec<(ImageKey, PathBuf, u8)> {
    let mut output = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        rest = &rest[open..];
        if rest.starts_with("<!--") {
            let Some(end) = rest.find("-->") else {
                break;
            };
            rest = &rest[end + 3..];
            continue;
        }
        let Some(end) = tag_end(rest) else {
            break;
        };
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        if !tag.starts_with("record")
            || !tag
                .as_bytes()
                .get(6)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b'/')
        {
            continue;
        }
        let attrs = attributes(&tag[6..]);
        let (Some(from), Some(to)) = (attrs.get("from"), attrs.get("to")) else {
            continue;
        };
        let Some((key, quality)) = mapping_target(to) else {
            continue;
        };
        let Some(stem) = relative_source(config, from) else {
            continue;
        };
        output.push((key, stem, quality));
        if output.len() >= MAX_MAPPINGS {
            break;
        }
    }
    output
}

fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (i, byte) in text.bytes().enumerate().skip(1) {
        if quote == Some(byte) {
            quote = None;
        } else if quote.is_none() && (byte == b'\'' || byte == b'"') {
            quote = Some(byte);
        } else if quote.is_none() && byte == b'>' {
            return Some(i);
        }
    }
    None
}

fn attributes(mut text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    loop {
        text = text.trim_start();
        let key_len = text
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
            .count();
        if key_len == 0 {
            break;
        }
        let key = &text[..key_len];
        text = text[key_len..].trim_start();
        if !text.starts_with('=') {
            break;
        }
        text = text[1..].trim_start();
        let Some(quote) = text.chars().next().filter(|c| *c == '\'' || *c == '"') else {
            break;
        };
        text = &text[1..];
        let Some(end) = text.find(quote) else {
            break;
        };
        values.insert(key.to_owned(), xml_unescape(&text[..end]));
        text = &text[end + 1..];
    }
    values
}

fn xml_unescape(text: &str) -> String {
    // &amp; last prevents double interpretation of a literal &quot; filename.
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn mapping_target(target: &str) -> Option<(ImageKey, u8)> {
    let normalized = target.replace('\\', "/").to_lowercase();
    let parts: Vec<_> = normalized.split('/').collect();
    if parts.len() < 5 || parts[0] != "graphics" || parts[1] != "pictures" {
        return None;
    }
    let id = parts[3]
        .strip_prefix("r-")
        .unwrap_or(parts[3])
        .parse::<u32>()
        .ok()?;
    if id == 0 {
        return None;
    }
    match (parts[2], parts[4], parts.len()) {
        ("person", "portrait", 5) => Some((
            ImageKey {
                kind: ImageKind::Portrait,
                id,
            },
            0,
        )),
        ("club", "logo", 5) => Some((
            ImageKey {
                kind: ImageKind::Badge,
                id,
            },
            0,
        )),
        ("club", "icon", 5) => Some((
            ImageKey {
                kind: ImageKind::Badge,
                id,
            },
            1,
        )),
        ("club", "logo", 6) if parts[5] == "small" => Some((
            ImageKey {
                kind: ImageKind::Badge,
                id,
            },
            1,
        )),
        _ => None,
    }
}

fn relative_source(config: &Path, source: &str) -> Option<PathBuf> {
    let source = source.replace('\\', "/");
    if source.contains(':') || source.starts_with('/') || source.contains('\0') {
        return None;
    }
    let path = Path::new(&source);
    if path
        .components()
        .any(|p| !matches!(p, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(config.parent()?.join(path))
}

fn decode_image(stem: &Path) -> Option<ColorImage> {
    let mut options = vec![stem.to_owned()];
    if stem.extension().is_none() {
        for ext in ["png", "jpg", "jpeg", "webp"] {
            options.push(stem.with_extension(ext));
        }
    }
    let path = options
        .into_iter()
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= MAX_IMAGE_BYTES))?;
    let bytes = fs::read(path).ok()?;
    let mut decoder = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    decoder.limits(limits);
    let decoded = decoder.decode().ok()?.thumbnail(512, 512).to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [decoded.width() as usize, decoded.height() as usize],
        decoded.as_raw(),
    ))
}

/// Local path index for the native WPF client. Shares the same verified graphics
/// mapping priority as egui, without decoding pixels or creating a GPU context.
pub struct NativeImagePaths {
    mappings: HashMap<ImageKey, Mapping>,
}
impl NativeImagePaths {
    pub fn load() -> Self {
        Self::load_with_defaults(default_roots())
    }
    pub fn load_for_profile(profile: &str) -> Self {
        Self::load_with_defaults(default_roots_for_profile(profile))
    }
    fn load_with_defaults(defaults: Vec<PathBuf>) -> Self {
        let configured = settings_path()
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .map(|s| s.roots)
            .unwrap_or_default();
        // Explicit application settings remain the user's override. Only the
        // automatically discovered defaults are selected by game edition.
        let roots = if configured.is_empty() { defaults } else { configured };
        Self { mappings: build_index(&roots).0 }
    }
    pub fn path(&self, kind: EntityKind, id: u32) -> Option<PathBuf> {
        let stem = &self.mappings.get(&ImageKey::from_entity(kind, id)?)?.stem;
        let mut paths = vec![stem.clone()];
        if stem.extension().is_none() {
            paths.extend(["png", "jpg", "jpeg", "webp"].map(|ext| stem.with_extension(ext)));
        }
        paths.into_iter().find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= MAX_IMAGE_BYTES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conventional_graphics_roots_are_isolated_by_connected_edition() {
        let home = PathBuf::from("fixture-home");
        let fm27 = conventional_roots([home.clone()], Some("FM27 27.0.0 / fingerprint"));
        let fm26 = conventional_roots([home.clone()], Some("FM26 26.3.2 / fingerprint"));
        assert_eq!(fm27.len(), 2);
        assert_eq!(fm26.len(), 2);
        assert!(fm27.iter().all(|p| p.to_string_lossy().contains("27")));
        assert!(fm26.iter().all(|p| p.to_string_lossy().contains("26")));
        assert!(!fm27.iter().any(|p| fm26.contains(p)));
        for unknown in ["", "FM270", "unsupported FM27", "snapshot"] {
            assert!(conventional_roots([home.clone()], Some(unknown)).is_empty());
        }
        assert!(conventional_roots([home], None)[0].to_string_lossy().contains("Football Manager 27"));
    }

    #[test]
    fn same_uid_in_two_editions_resolves_only_in_selected_graphics_root() {
        let folder = std::env::temp_dir().join(format!("touchline-graphics-edition-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let roots27 = conventional_roots([folder.clone()], Some("FM27 test"));
        let roots26 = conventional_roots([folder.clone()], Some("FM26 test"));
        for (roots, file) in [(&roots27, "new"), (&roots26, "old")] {
            fs::create_dir_all(&roots[0]).unwrap();
            fs::write(roots[0].join("config.xml"), format!("<record from=\"{file}\" to=\"graphics/pictures/person/2000000001/portrait\"/>")).unwrap();
            fs::write(roots[0].join(format!("{file}.png")), b"fixture").unwrap();
        }
        let current = NativeImagePaths { mappings: build_index(&roots27).0 };
        let previous = NativeImagePaths { mappings: build_index(&roots26).0 };
        assert_eq!(current.path(EntityKind::Player, 2_000_000_001).unwrap(), roots27[0].join("new.png"));
        assert_eq!(previous.path(EntityKind::Player, 2_000_000_001).unwrap(), roots26[0].join("old.png"));
        fs::remove_file(roots27[0].join("new.png")).unwrap();
        assert!(current.path(EntityKind::Player, 2_000_000_001).is_none(), "must not fall back to the other game's portrait");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn maps_person_regen_club_and_ignores_other_resources() {
        let xml = r#"<record><list id="maps">
            <record from="2000276500" to="graphics/pictures/person/2000276500/portrait"/>
            <record from='r-2002103449.png' to='graphics/pictures/person/r-2002103449/portrait'/>
            <record from="1141" to="graphics/pictures/club/1141/logo"/>
            <record from="1141s" to="graphics/pictures/club/1141/icon"/>
            <record from="wrong" to="graphics/pictures/person/2000276500/icon"/>
            <!-- <record from="old" to="graphics/pictures/person/88/portrait"/> -->
        </list></record>"#;
        let result = parse_config(Path::new("pack/config.xml"), xml);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].0.id, 2_000_276_500);
        assert_eq!(result[1].0.id, 2_002_103_449);
        assert_eq!(result[2].0.kind, ImageKind::Badge);
        assert_eq!(result[2].2, 0);
        assert_eq!(result[3].2, 1);
    }

    #[test]
    fn source_paths_are_local_relative_and_entities_are_decoded_once() {
        let config = Path::new("pack/config.xml");
        assert_eq!(
            relative_source(config, "faces/player.png"),
            Some(Path::new("pack/faces/player.png").to_owned())
        );
        for bad in [
            "../escape",
            "C:/other/file",
            "/root/file",
            "\\\\server\\file",
            "https://example.com/img",
        ] {
            assert!(relative_source(config, bad).is_none(), "{bad}");
        }
        let parsed = parse_config(
            config,
            r#"<record from="Smith &amp; Jones" to="graphics/pictures/person/88/portrait"/>"#,
        );
        assert!(parsed[0].1.ends_with("Smith & Jones"));
        assert_eq!(xml_unescape("&amp;quot;"), "&quot;");
    }

    #[test]
    fn current_pack_priority_and_full_badge_priority_are_deterministic() {
        let a = Mapping {
            stem: PathBuf::from("new"),
            priority: (0, 0, "faces/000 lecce/config.xml".into()),
        };
        let old = Mapping {
            stem: PathBuf::from("old"),
            priority: (0, 0, "faces/009 gunzo/config.xml".into()),
        };
        let lower_root = Mapping {
            stem: PathBuf::from("other"),
            priority: (1, 0, "000/config.xml".into()),
        };
        let icon = Mapping {
            stem: PathBuf::from("icon"),
            priority: (0, 1, "000/config.xml".into()),
        };
        assert!(a.priority < old.priority);
        assert!(a.priority < lower_root.priority);
        assert!(a.priority < icon.priority);
    }

    #[test]
    fn missing_assets_do_not_produce_fabricated_images() {
        assert!(decode_image(Path::new("missing-not-a-real-asset-file.png")).is_none());
        assert!(ImageKey::from_entity(EntityKind::City, 1141).is_none());
    }

    #[test]
    #[ignore = "requires TOUCHLINE_TEST_PORTRAIT_IDS and locally installed FM graphics"]
    fn installed_graphics_are_found_and_decodable() {
        let raw_ids = std::env::var("TOUCHLINE_TEST_PORTRAIT_IDS")
            .expect("Set comma-separated portrait UIDs for your local graphics pack");
        let ids: Vec<u32> = raw_ids.split(',').map(|v| v.trim().parse().expect("portrait UID")).collect();
        assert!(!ids.is_empty());
        let (maps, _, _) = build_index(&default_roots());
        for id in ids {
            let entry = maps.get(&ImageKey { kind: ImageKind::Portrait, id })
                .expect("installed asset mapping");
            let image = decode_image(&entry.stem).expect("decode installed image");
            assert!(image.size[0] > 50 && image.size[1] > 50);
        }
    }
}
