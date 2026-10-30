//! Cheap, read-only save-load observation. No registry scan or FM writes occur here.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub pid: u32,
    pub created: u64,
    pub module_base: u64,
    pub module_size: u32,
    pub module_stamp: u32,
    pub db: u64,
    pub registry_begin: u64,
    pub registry_end: u64,
    // A reload can rebuild people while retaining the same vector allocation.
    // Include native object identity, not only the vector header or game date.
    pub anchors: Vec<(u32, u64, u64, u32)>,
}
impl Identity {
    pub fn matches_session(&self, session: &Session) -> bool {
        self.pid == session.pid && self.created == session.created
            && self.module_base == session.module_base && self.module_size == session.module_size
            && self.module_stamp == session.module_stamp && self.db == session.db
            && self.registry_begin == session.registry_begin
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub identity: Identity,
    pub game_date: String,
}
#[derive(Clone, Debug)]
pub enum Observation {
    Ready(Ready),
    Waiting(String),
    Error(String),
    UnsupportedBuild(String),
    Absent,
}

/// A known startup/load state is distinct from denied access or an invalid
/// object layout. Only these states should be described as waiting for a save.
#[derive(Debug)]
struct NotReady(&'static str);
impl std::fmt::Display for NotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.0) }
}
impl std::error::Error for NotReady {}
fn require_ready(condition: bool, reason: &'static str) -> Result<()> {
    if !condition { return Err(anyhow::anyhow!(NotReady(reason))); }
    Ok(())
}

fn is_exited(pid: u32) -> bool {
    use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION}};
    // A terminated FM can remain in the process inventory while another
    // program retains a handle. Do not let that zombie block a new instance.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return false; }
        let mut code = 259u32; // STILL_ACTIVE
        let read = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        read != 0 && code != 259
    }
}

fn probe_pid(pid: u32) -> Result<Ready> {
    let process = Process::open(pid, false)?;
    let module = process.modules()?.into_iter()
        .find(|m| m.name.eq_ignore_ascii_case("game_plugin.dll"))
        .ok_or_else(|| anyhow::anyhow!(NotReady("게임 데이터 모듈이 아직 없습니다. FM 시작이 완료될 때까지 기다려 주세요")))?;
    let build=detect_build(&process,&module)?;
    let profile=require_read_profile(&build)?;
    let layout=profile.require_read_layout()?;
    let (stamp,size)=(build.timestamp,build.image_size);
    let created = process.created()?;
    let db = process.u64(module.base + layout.database_rva)?;
    require_ready(db != 0, "세이브 데이터가 아직 없습니다. FM에서 세이브를 열어 주세요")?;
    ensure!(pointer(db) == db, "세이브 데이터 주소 검증 실패");
    let (begin, end) = registry_with_layout(&process, module.base, db,layout)?;
    let count = (end - begin) / 8;
    let mut positions = vec![0, count / 4, count / 2, count * 3 / 4, count - 1];
    positions.sort_unstable();
    positions.dedup();
    let mut anchors = Vec::new();
    for index in positions {
        let address = process.u64(begin + index * 8)?;
        if address == 0 {
            anchors.push((index as u32, 0, 0, 0));
            continue;
        }
        ensure!(pointer(address) == address, "인물 목록이 로드 중입니다");
        let raw = process.read(address, 16)?;
        let vtable = u64::from_le_bytes(raw[..8].try_into().unwrap());
        let native_index = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        let uid = u32::from_le_bytes(raw[12..16].try_into().unwrap());
        require_ready(native_index as u64 == index && uid > 0, "인물 목록 구성이 변경 중입니다")?;
        let (_,type_name)=type_info(&process, module.base, size, vtable)?;
        ensure!(is_person_registry_type(&type_name),"인물 레지스트리 객체 유형이 다릅니다: {type_name}");
        anchors.push((index as u32, address, vtable, uid));
    }
    require_ready(anchors.iter().any(|(_, address, _, _)| *address != 0), "인물 목록이 아직 비어 있습니다")?;
    let date = decode_date(process.u32(module.base + layout.date_rva)?).context("게임 날짜를 아직 확인할 수 없습니다")?;
    require_ready(process.created()? == created && process.u64(module.base + layout.database_rva)? == db
        && registry_with_layout(&process, module.base, db,layout)? == (begin, end), "조회 중 세이브 구성이 변경됐습니다")?;
    for &(index, address, vtable, uid) in &anchors {
        require_ready(process.u64(begin + index as u64 * 8)? == address, "조회 중 인물 목록이 변경됐습니다")?;
        if address != 0 {
            require_ready(process.u64(address)? == vtable && process.u32(address + 8)? == index
                && process.u32(address + 12)? == uid, "조회 중 인물 객체가 변경됐습니다")?;
        }
    }
    Ok(Ready { identity: Identity { pid, created, module_base: module.base, module_size: size,
        module_stamp: stamp, db, registry_begin: begin, registry_end: end, anchors }, game_date: date_text(date) })
}

fn error_observation(error:anyhow::Error)->Observation {
    if error.downcast_ref::<profiles::UnsupportedBuild>().is_some() {
        Observation::UnsupportedBuild(format!("{error:#}"))
    } else if error.downcast_ref::<NotReady>().is_some() || error.downcast_ref::<EmptyRegistry>().is_some() {
        Observation::Waiting(format!("{error:#}"))
    } else {
        Observation::Error(format!("{error:#}"))
    }
}

/// Prefer the attached FM process while it exists; never silently select a
/// different game just because the attached process is loading its save.
pub fn probe(preferred_pid: Option<u32>) -> Observation {
    let candidates = match memory::processes() {
        Ok(items) => items.into_iter().filter(|(_, name)| name.eq_ignore_ascii_case("fm.exe"))
            .map(|(pid, _)| pid).filter(|pid| !is_exited(*pid)).collect::<Vec<_>>(),
        Err(error) => return Observation::Error(format!("FM 프로세스 상태를 확인하지 못했습니다: {error:#}")),
    };
    if candidates.is_empty() { return Observation::Absent; }
    if let Some(pid) = preferred_pid.filter(|pid| candidates.contains(pid)) {
        return match probe_pid(pid) {
            Ok(ready) => Observation::Ready(ready),
            Err(error) => error_observation(error),
        };
    }
    let mut ready=Vec::new();
    let mut unsupported=Vec::new();
    let mut waiting=Vec::new();
    let mut errors=Vec::new();
    for pid in candidates {
        match probe_pid(pid) {
            Ok(state)=>ready.push(state),
            Err(error)=>match error_observation(error) {
                Observation::UnsupportedBuild(reason)=>unsupported.push(format!("PID {pid}: {reason}")),
                Observation::Waiting(reason)=>waiting.push(reason),
                Observation::Error(reason)=>errors.push(format!("PID {pid}: {reason}")),
                _=>unreachable!(),
            },
        }
    }
    match ready.len() {
        1 => Observation::Ready(ready.into_iter().next().unwrap()),
        0 if !unsupported.is_empty()=>Observation::UnsupportedBuild(unsupported.join("\n")),
        0 if !errors.is_empty()=>Observation::Error(errors.join("\n")),
        0 => Observation::Waiting(waiting.into_iter().next().unwrap_or_else(||"FM은 실행 중이며 세이브 로드 완료를 기다립니다".into())),
        _ => Observation::Error("실행 중인 FM 세션이 여러 개입니다. 연결할 게임 하나만 남겨 주세요".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_build_is_not_reported_as_loading() {
        let error=anyhow::anyhow!(profiles::UnsupportedBuild {timestamp:profiles::FM27.timestamp,
            image_size:profiles::FM27.image_size,profile:Some(&profiles::FM27)});
        assert!(matches!(error_observation(error),Observation::UnsupportedBuild(_)));
        assert!(matches!(error_observation(anyhow::anyhow!(NotReady("세이브 로드 중"))),Observation::Waiting(_)));
    }
    #[test]
    fn access_and_validation_errors_are_not_loading_and_keep_the_error_chain() {
        let error=anyhow::anyhow!("Access is denied (os error 5)").context("FM 프로세스 접근 실패");
        let Observation::Error(reason)=error_observation(error) else {panic!("access failure was hidden as loading")};
        assert!(reason.contains("FM 프로세스 접근 실패") && reason.contains("os error 5"));
        assert!(matches!(error_observation(anyhow::anyhow!("객체 RTTI 검증 실패")),Observation::Error(_)));
        assert!(matches!(error_observation(anyhow::anyhow!("인물 레지스트리 범위 검증 실패")),Observation::Error(_)));
        assert!(matches!(error_observation(anyhow::anyhow!(EmptyRegistry)),Observation::Waiting(_)));
        let error=anyhow::anyhow!("목록 범위 오류").context(NotReady("세이브 로드 중"));
        let Observation::Waiting(reason)=error_observation(error) else {panic!("load wait was not retained")};
        assert!(reason.contains("세이브 로드 중") && reason.contains("목록 범위 오류"));
    }
    #[test]
    #[ignore = "requires a running supported FM save; read-only, no debugger or game control"]
    fn live_readonly_probe() {
        let started = std::time::Instant::now();
        let observed=probe(None);
        let Observation::Ready(ready) = observed else { panic!("No stable supported FM save is ready: {observed:?}") };
        eprintln!("{}", serde_json::json!({"read_only":true,"game_values_changed":false,
            "pid":ready.identity.pid,"game_date":ready.game_date,"anchors":ready.identity.anchors.len(),
            "registry_count":(ready.identity.registry_end-ready.identity.registry_begin)/8,
            "elapsed_ms":started.elapsed().as_millis()}));
    }
}
