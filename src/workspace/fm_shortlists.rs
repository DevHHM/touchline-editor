//! Bounded file interchange. Never changes a running game's shortlist vectors.
//! Evidence/version limitations: reference/fm27-shortlist-files.md.
use super::{Library, Ref, Shortlist};
use crate::model::EntityKind;
use anyhow::{Context, Result, bail, ensure};
use ctr::cipher::{KeyIvInit, StreamCipher};
use std::{
    collections::HashSet,
    io::{Read, Write},
    path::Path,
};

const MAX_FILE: usize = 32 * 1024 * 1024;
const MAX_INDEX: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_LIST: usize = 10000;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("명단 길이 오류")?;
        let out = self
            .bytes
            .get(self.pos..end)
            .context("관심명단 파일이 잘렸습니다")?;
        self.pos = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn count(&mut self, max: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        ensure!(
            n <= max,
            "관심명단의 항목 수 또는 길이가 지원 범위를 넘습니다"
        );
        Ok(n)
    }
    fn string(&mut self) -> Result<String> {
        let n = self.count(4096)?;
        Ok(std::str::from_utf8(self.take(n)?)
            .context("지원하지 않는 명단 문자열 인코딩")?
            .to_owned())
    }
    fn utf16_string(&mut self) -> Result<String> {
        let units = self.count(2048)?;
        let words: Vec<u16> = self
            .take(units * 2)?
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        ensure!(
            self.take(2)? == [0, 0],
            "구형 관심명단 문자열의 끝 표시가 잘못됐습니다"
        );
        String::from_utf16(&words).context("지원하지 않는 UTF-16 명단 문자열")
    }
    fn end(&self) -> Result<()> {
        ensure!(
            self.pos == self.bytes.len(),
            "지원하지 않는 추가 명단 데이터가 있습니다"
        );
        Ok(())
    }
}
fn put_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}
fn checked(list: &Shortlist, kind: EntityKind) -> Result<()> {
    ensure!(
        matches!(kind, EntityKind::Player | EntityKind::Staff),
        "FM 관심명단은 선수 또는 스태프만 지원합니다"
    );
    Library {
        lists: vec![list.clone()],
        presets: vec![],
    }
    .validate()?;
    ensure!(
        list.members
            .iter()
            .all(|r| r.kind == kind && r.id <= i32::MAX as u32),
        "선수와 스태프는 별도 명단으로 내보내세요. 다른 종류의 대상 또는 지원 범위를 넘는 UID가 있습니다"
    );
    Ok(())
}

/// Legacy SI shortlist payload used by observed FMRTE FMF files (.slf / .ssf).
pub fn encode_payload(list: &Shortlist, kind: EntityKind) -> Result<Vec<u8>> {
    checked(list, kind)?;
    let mut out = vec![3, 1];
    out.extend_from_slice(if kind == EntityKind::Player {
        b"fls"
    } else {
        b"fss"
    });
    out.extend_from_slice(&0x022eu16.to_le_bytes());
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // No save-specific creation date.
    put_string(&mut out, &list.name);
    out.extend_from_slice(&(list.members.len() as u32).to_le_bytes());
    for m in &list.members {
        out.extend_from_slice(&m.id.to_le_bytes());
    }
    out.push(0);
    Ok(out)
}
pub fn decode_payload(bytes: &[u8]) -> Result<Shortlist> {
    let mut r = Reader::new(bytes);
    ensure!(r.take(2)? == [3, 1], "지원하지 않는 SLF/SSF 버전입니다");
    let kind = match r.take(3)? {
        b"fls" => EntityKind::Player,
        b"fss" => EntityKind::Staff,
        _ => bail!("선수·스태프 관심명단 파일이 아닙니다"),
    };
    let version = u16::from_le_bytes(r.take(2)?.try_into()?);
    let flags = u16::from_le_bytes(r.take(2)?.try_into()?);
    ensure!(
        [0x12e, 0x22e].contains(&version) && [0, 0x100].contains(&flags),
        "지원하지 않는 관심명단 헤더입니다"
    );
    let _date = r.u32()?;
    let name = if version == 0x12e {
        r.utf16_string()?
    } else {
        r.string()?
    };
    let count = r.count(MAX_LIST)?;
    let mut members = Vec::with_capacity(count);
    let mut seen = HashSet::new();
    for _ in 0..count {
        let id = r.u32()?;
        ensure!(id > 0 && id <= i32::MAX as u32, "잘못된 인물 UID입니다");
        if seen.insert(id) {
            members.push(Ref { kind, id });
        }
    }
    ensure!(r.u8()? == 0, "지원하지 않는 관심명단 후속 자료입니다");
    r.end()?;
    let list = Shortlist { name, members };
    checked(&list, kind)?;
    Ok(list)
}

struct Entry {
    path: String,
    offset: usize,
    stored: usize,
    original: usize,
}
fn directory(
    r: &mut Reader<'_>,
    prefix: &str,
    depth: usize,
    entries: &mut Vec<Entry>,
    budget: &mut usize,
) -> Result<()> {
    ensure!(depth < 8, "FMF 폴더 깊이가 너무 깊습니다");
    ensure!(*budget > 0, "FMF 폴더가 너무 많습니다");
    *budget -= 1;
    let name = r.string()?;
    let prefix = if depth == 0 {
        String::new()
    } else {
        format!("{prefix}{name}/")
    };
    let count = r.count(MAX_ENTRIES)?;
    ensure!(
        entries.len() + count <= MAX_ENTRIES,
        "FMF 항목이 너무 많습니다"
    );
    for _ in 0..count {
        let name = r.string()?;
        let extension = r.string()?;
        let offset = usize::try_from(r.u64()?)?;
        let stored = usize::try_from(r.u64()?)?;
        let original = usize::try_from(r.u64()?)?;
        ensure!(
            stored <= MAX_FILE && original <= MAX_FILE,
            "FMF 항목이 너무 큽니다"
        );
        r.take(16)?; // archive timestamps; never filesystem paths to extract
        entries.push(Entry {
            path: format!("{prefix}{name}{extension}"),
            offset,
            stored,
            original,
        });
    }
    let count = r.count(MAX_ENTRIES)?;
    for _ in 0..count {
        directory(r, &prefix, depth + 1, entries, budget)?;
    }
    Ok(())
}
fn inflate(bytes: &[u8], max: usize) -> Result<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::new(bytes).context("FMF 압축 형식 오류")?;
    decoder.window_log_max(22)?; // FM27 entries use a 4 MiB history window, even for tiny payloads.
    let mut out = vec![];
    decoder.take((max + 1) as u64).read_to_end(&mut out)?;
    ensure!(out.len() <= max, "FMF 압축 해제 크기 제한을 초과했습니다");
    Ok(out)
}
fn deflate(bytes: &[u8], window_log: u32) -> Result<Vec<u8>> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3)?;
    encoder.window_log(window_log)?;
    encoder.include_checksum(true)?;
    encoder.include_contentsize(false)?;
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}
fn entry_bytes(
    bytes: &[u8],
    e: &Entry,
    encrypted: bool,
    compressed: bool,
    index_start: usize,
) -> Result<Vec<u8>> {
    let start = 26usize.checked_add(e.offset).context("FMF 위치 오류")?;
    let end = start.checked_add(e.stored).context("FMF 길이 오류")?;
    ensure!(
        start >= 26 && end <= index_start,
        "FMF 항목이 색인 영역과 겹칩니다"
    );
    let data = bytes.get(start..end).context("FMF 항목 잘림")?;
    let out = if encrypted {
        let mut r = Reader::new(data);
        ensure!(
            r.u32()? == 16 && r.u32()? == 16,
            "지원하지 않는 FMF 항목 암호 형식입니다"
        );
        let key = r.take(16)?;
        let iv = r.take(16)?;
        let mut out = r.take(data.len() - r.pos)?.to_vec();
        let mut cipher = ctr::Ctr128BE::<aes::Aes128>::new_from_slices(key, iv)
            .map_err(|_| anyhow::anyhow!("FMF 키 형식 오류"))?;
        cipher.apply_keystream(&mut out);
        out
    } else if compressed {
        inflate(data, MAX_INDEX)?
    } else {
        data.to_vec()
    };
    ensure!(out.len() == e.original, "FMF 원본 크기가 일치하지 않습니다");
    Ok(out)
}
pub fn decode_archive(bytes: &[u8]) -> Result<Library> {
    ensure!(bytes.len() <= MAX_FILE, "FMF 파일이 너무 큽니다");
    let mut r = Reader::new(bytes);
    ensure!(r.take(2)? == [2, 1], "지원하지 않는 FMF 버전입니다");
    let magic = r.take(3)?;
    ensure!(matches!(magic, b"fmf" | b"afe"), "FMF 파일이 아닙니다");
    ensure!(
        r.take(4)? == [0x2e, 8, 0, 0],
        "지원하지 않는 FMF 헤더입니다"
    );
    let index_start = (r.u32()? as usize)
        .checked_add(9)
        .context("FMF 색인 위치 오류")?;
    ensure!(r.u32()? == 0, "지원하지 않는 대용량 FMF 파일입니다");
    let compression = r.u8()?;
    r.take(8)?;
    let encrypted = magic == b"afe" && compression == 17;
    ensure!(
        encrypted || (magic == b"fmf" && [0, 17].contains(&compression)),
        "지원하지 않는 FMF 저장 방식입니다"
    );
    ensure!(
        index_start >= 26 && index_start < bytes.len(),
        "FMF 색인 위치 오류"
    );
    let mut ix = Reader::new(&bytes[index_start..]);
    ensure!(
        ix.take(9)? == [2, 1, b'f', b'm', b'f', 0x2e, 8, 0, 0],
        "FMF 색인 헤더 오류"
    );
    let compressed_size = if ix.bytes[ix.pos..].starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let remaining = ix.bytes.len() - ix.pos;
        ensure!(remaining <= MAX_INDEX, "FMF 색인이 너무 큽니다");
        remaining
    } else {
        ix.count(MAX_INDEX)?
    };
    let index = inflate(ix.take(compressed_size)?, MAX_INDEX)?;
    ix.end()?;
    let mut r = Reader::new(&index);
    let mut entries = vec![];
    let mut budget = MAX_ENTRIES;
    directory(&mut r, "", 0, &mut entries, &mut budget)?;
    r.end()?;
    // Validate even entries we do not import; never extract archive paths.
    for e in &entries {
        let end = 26usize
            .checked_add(e.offset)
            .and_then(|v| v.checked_add(e.stored))
            .context("FMF 항목 위치 오류")?;
        ensure!(end <= index_start, "FMF 항목이 색인 영역과 겹칩니다");
    }
    let mut library = Library::default();
    for e in entries
        .iter()
        .filter(|e| e.path.ends_with(".slf") || e.path.ends_with(".ssf"))
    {
        let data = unpack_payload(entry_bytes(
            bytes,
            e,
            encrypted,
            compression == 17,
            index_start,
        )?)?;
        let list = decode_payload(&data)?;
        ensure!(
            (data.get(2..5) == Some(b"fss")) == e.path.ends_with(".ssf"),
            "FMF 확장자와 명단 종류가 일치하지 않습니다"
        );
        library.lists.push(list);
    }
    ensure!(
        !library.lists.is_empty(),
        "FMF 파일에 지원하는 관심명단이 없습니다"
    );
    library.validate()?;
    Ok(library)
}

fn write_entry_index(
    out: &mut Vec<u8>,
    name: &str,
    ext: &str,
    offset: usize,
    stored: usize,
    original: usize,
) {
    put_string(out, name);
    put_string(out, ext);
    for value in [offset, stored, original] {
        out.extend_from_slice(&(value as u64).to_le_bytes());
    }
    out.extend_from_slice(&[0; 16]);
}
fn metadata(kind: EntityKind) -> Vec<u8> {
    // AOM v4 uses ASCII. The actual Korean list name is in the UTF-8 SLF/SSF.
    let mut out = vec![3, 1, b'm', b'o', b'a', b'.', 4, 0];
    put_string(
        &mut out,
        if kind == EntityKind::Player {
            "PLAYER_SHORTLIST_TYPE_HANDLER"
        } else {
            "STAFF_SHORTLIST_TYPE_HANDLER"
        },
    );
    put_string(&mut out, "Touchline");
    put_string(&mut out, "");
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&[0xff; 16]);
    put_string(&mut out, ""); // No author account or Steam ID exported.
    out.extend_from_slice(&[2, 0, 1, 0, 2]);
    out.extend_from_slice(&[0; 17]);
    out
}
fn unpack_payload(bytes: Vec<u8>) -> Result<Vec<u8>> {
    if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        inflate(&bytes, MAX_INDEX)
    } else {
        Ok(bytes)
    }
}
/// FM27 shipped resource layout: Zstd entries and direct Zstd index.
/// No proprietary template, thumbnail or executable is copied.
pub fn encode_archive(list: &Shortlist, kind: EntityKind) -> Result<Vec<u8>> {
    let payload = encode_payload(list, kind)?;
    let metadata = metadata(kind);
    let packed_payload = deflate(&payload, 22)?;
    let packed_metadata = deflate(&metadata, 22)?;
    let mut index = vec![];
    put_string(&mut index, "Touchline");
    index.extend_from_slice(&1u32.to_le_bytes());
    write_entry_index(
        &mut index,
        "Touchline",
        if kind == EntityKind::Player {
            ".slf"
        } else {
            ".ssf"
        },
        0,
        packed_payload.len(),
        payload.len(),
    );
    index.extend_from_slice(&1u32.to_le_bytes());
    put_string(&mut index, "_data");
    index.extend_from_slice(&1u32.to_le_bytes());
    write_entry_index(
        &mut index,
        "details",
        ".aom",
        packed_payload.len(),
        packed_metadata.len(),
        metadata.len(),
    );
    index.extend_from_slice(&0u32.to_le_bytes());
    let compressed = deflate(&index, 20)?;
    let mut out = vec![2, 1, b'f', b'm', b'f', 0x2e, 8, 0, 0];
    out.extend_from_slice(
        &((26 + packed_payload.len() + packed_metadata.len() - 9) as u32).to_le_bytes(),
    );
    out.extend_from_slice(&[0; 13]);
    out[17] = 17;
    out[25] = 3;
    out.extend_from_slice(&packed_payload);
    out.extend_from_slice(&packed_metadata);
    out.extend_from_slice(&[2, 1, b'f', b'm', b'f', 0x2e, 8, 0, 0]);
    out.extend_from_slice(&compressed);
    Ok(out)
}
pub fn read(path: &Path) -> Result<Library> {
    ensure!(path.is_file(), "가져올 관심명단 파일이 없습니다");
    ensure!(
        path.metadata()?.len() <= MAX_FILE as u64,
        "관심명단 파일은 최대 32MB입니다"
    );
    let mut bytes = vec![];
    std::fs::File::open(path)?
        .take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_FILE, "관심명단 파일이 너무 큽니다");
    let bytes = unpack_payload(bytes)?;
    if bytes.starts_with(&[3, 1]) {
        Ok(Library {
            lists: vec![decode_payload(&bytes)?],
            presets: vec![],
        })
    } else {
        decode_archive(&bytes)
    }
}
pub fn export(path: &Path, list: &Shortlist, kind: EntityKind) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        ext == "fmf"
            || (ext == "slf" && kind == EntityKind::Player)
            || (ext == "ssf" && kind == EntityKind::Staff),
        "명단 종류에 맞는 .fmf / .slf / .ssf 확장자를 선택하세요"
    );
    let bytes = if ext == "fmf" {
        encode_archive(list, kind)?
    } else {
        encode_payload(list, kind)?
    };
    // create_new prevents silently replacing a file another editor/game just saved.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("새 파일 경로를 선택하세요. 기존 파일은 덮어쓰지 않습니다")?;
    let result = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result.context("관심명단 파일 저장 실패")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn list(kind: EntityKind) -> Shortlist {
        Shortlist {
            name: "영입 후보 🏆".into(),
            members: vec![
                Ref { kind, id: 101 },
                Ref {
                    kind,
                    id: 2000123456,
                },
            ],
        }
    }
    #[test]
    fn payload_and_archive_round_trip_player_staff_and_empty() {
        for kind in [EntityKind::Player, EntityKind::Staff] {
            let mut l = list(kind);
            for _ in 0..2 {
                let a = decode_payload(&encode_payload(&l, kind).unwrap()).unwrap();
                let b = decode_archive(&encode_archive(&l, kind).unwrap()).unwrap();
                assert_eq!(a.name, l.name);
                assert_eq!(a.members, l.members);
                assert_eq!(b.lists[0].members, l.members);
                assert_eq!(b.lists[0].name, l.name);
                l.members.clear();
            }
        }
    }
    #[test]
    fn mixed_and_non_people_are_rejected() {
        let mut l = list(EntityKind::Player);
        l.members[1].kind = EntityKind::Staff;
        assert!(encode_archive(&l, EntityKind::Player).is_err());
        l.members[1].kind = EntityKind::Club;
        assert!(encode_payload(&l, EntityKind::Player).is_err());
    }
    #[test]
    fn old_utf16_names_and_malformed_terminators() {
        let mut bytes = vec![3, 1, b'f', b'l', b's', 0x2e, 1, 0, 1, 0, 0, 0, 0];
        let name: Vec<u16> = "Old 명단".encode_utf16().collect();
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        for c in name {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let terminator = bytes.len();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&101u32.to_le_bytes());
        bytes.extend_from_slice(&2000123456u32.to_le_bytes());
        bytes.push(0);
        let l = decode_payload(&bytes).unwrap();
        assert_eq!(l.name, "Old 명단");
        assert_eq!(l.members, list(EntityKind::Player).members);
        bytes[terminator] = 1;
        assert!(decode_payload(&bytes).is_err());
    }
    #[test]
    fn encrypted_entries_and_compressed_payloads() {
        let l = list(EntityKind::Staff);
        let payload = encode_payload(&l, EntityKind::Staff).unwrap();
        let packed = zstd::stream::encode_all(payload.as_slice(), 3).unwrap();
        assert_eq!(
            decode_payload(&unpack_payload(packed).unwrap())
                .unwrap()
                .members,
            l.members
        );
        let key = [0x11u8; 16];
        let iv = [0x22u8; 16];
        let mut data = payload.clone();
        ctr::Ctr128BE::<aes::Aes128>::new_from_slices(&key, &iv)
            .unwrap()
            .apply_keystream(&mut data);
        let mut archive = vec![0; 26];
        archive.extend_from_slice(&16u32.to_le_bytes());
        archive.extend_from_slice(&16u32.to_le_bytes());
        archive.extend_from_slice(&key);
        archive.extend_from_slice(&iv);
        archive.extend_from_slice(&data);
        let e = Entry {
            path: "test.ssf".into(),
            offset: 0,
            stored: data.len() + 40,
            original: payload.len(),
        };
        assert_eq!(
            entry_bytes(&archive, &e, true, false, archive.len()).unwrap(),
            payload
        );
        archive[26] = 15;
        assert!(entry_bytes(&archive, &e, true, false, archive.len()).is_err());
    }
    #[test]
    fn decompressed_size_and_uid_limits() {
        let packed = zstd::stream::encode_all(vec![0u8; MAX_INDEX + 1].as_slice(), 1).unwrap();
        assert!(inflate(&packed, MAX_INDEX).is_err());
        let mut l = list(EntityKind::Player);
        l.members[0].id = u32::MAX;
        assert!(encode_payload(&l, EntityKind::Player).is_err());
    }
    #[test]
    fn truncations_and_wrong_magic_are_rejected_without_panics() {
        for bytes in [
            encode_payload(&list(EntityKind::Player), EntityKind::Player).unwrap(),
            encode_archive(&list(EntityKind::Staff), EntityKind::Staff).unwrap(),
        ] {
            for end in 0..bytes.len() {
                assert!(if bytes[0] == 3 {
                    decode_payload(&bytes[..end]).is_err()
                } else {
                    decode_archive(&bytes[..end]).is_err()
                });
            }
        }
        assert!(decode_archive(b"not an FM file").is_err());
    }
    #[test]
    fn observed_empty_slf_fixture_decodes() {
        // Byte-level schema fixture, no application code or art bundled.
        let mut bytes = vec![3, 1, b'f', b'l', b's', 0x2e, 2, 0, 1, 0xc6, 0x5b, 0x34, 0];
        put_string(&mut bytes, "Shortlist-12345");
        bytes.extend_from_slice(&[0; 5]);
        let l = decode_payload(&bytes).unwrap();
        assert_eq!(l.name, "Shortlist-12345");
        assert!(l.members.is_empty());
    }
    #[test]
    fn bounds_and_extra_data_are_rejected() {
        let mut bytes = encode_archive(&list(EntityKind::Player), EntityKind::Player).unwrap();
        bytes[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_archive(&bytes).is_err());
        let mut bytes = encode_payload(&list(EntityKind::Player), EntityKind::Player).unwrap();
        bytes.extend_from_slice(&[0]);
        assert!(decode_payload(&bytes).is_err());
    }
}
