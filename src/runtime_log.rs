//! Best-effort local operational diagnostics. Never records requests or game data.
use std::{collections::HashMap, fs::{self, File, OpenOptions}, io::{self, Write},
    path::PathBuf, sync::{Mutex, OnceLock}, time::{SystemTime, UNIX_EPOCH}};
#[cfg(test)]
use std::path::Path;
use std::os::windows::fs::OpenOptionsExt;
use serde_json::{Value, json};

const PART_BYTES:u64=256*1024;
const SOURCE_BYTES:u64=5*1024*1024;
const KEEP_PARTS:u32=4;
const KEEP_DAYS:u64=7*24*60*60;
static LOG:OnceLock<Mutex<Option<Logger>>>=OnceLock::new();

fn millis()->u128 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()}
fn session_id()->String {
    #[link(name="bcrypt")]
    unsafe extern "system" {fn BCryptGenRandom(h:*mut std::ffi::c_void,b:*mut u8,n:u32,flags:u32)->i32;}
    let mut bytes=[0u8;16];
    if unsafe {BCryptGenRandom(std::ptr::null_mut(),bytes.as_mut_ptr(),16,2)}!=0 {
        bytes=(millis().wrapping_shl(32)|u128::from(std::process::id())).to_le_bytes();
    }
    bytes.iter().map(|b|format!("{b:02x}")).collect()
}
fn utc_timestamp(ms:u128)->String {
    let seconds=(ms/1000).min(i64::MAX as u128) as i64;
    let z=seconds/86400+719468;
    let era=z/146097;let doe=z-era*146097;
    let yoe=(doe-doe/1460+doe/36524-doe/146096)/365;
    let mut year=yoe+era*400;let doy=doe-(365*yoe+yoe/4-yoe/100);
    let mp=(5*doy+2)/153;let day=doy-(153*mp+2)/5+1;
    let month=mp+if mp<10 {3}else{-9};year+=i64::from(month<=2);
    let s=seconds%86400;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",s/3600,s/60%60,s%60,ms%1000)
}
fn bounded(value:&str,max:usize)->String {value.chars().take(max).collect()}
fn replace_case(mut value:String,needle:&str)->String {
    if needle.is_empty(){return value;}
    let needle=needle.to_ascii_lowercase();
    while let Some(at)=value.to_ascii_lowercase().find(&needle) {
        value.replace_range(at..at+needle.len(),"[home]");
    }
    value
}
pub(crate) fn sanitize(value:&str)->String {
    sanitize_with_home(value,std::env::var("USERPROFILE").ok().as_deref())
}
fn sanitize_with_home(value:&str,home:Option<&str>)->String {
    let lower=value.to_ascii_lowercase();
    // Credential-bearing diagnostics are discarded as a whole. Authentication
    // failures remain useful through their static event/category and HTTP code.
    if ["bearer ","authorization:","authorization=","password", "api_key", "api-key",
        "access_token", "refresh_token", "secret=", "secret:", "token=", "token\"", "token:"]
        .iter().any(|key|lower.contains(key)) {return "[redacted credential-bearing text]".into();}
    if (lower.contains("http://")||lower.contains("https://"))&&lower.contains('@') {return "[redacted credential-bearing URL]".into();}
    if value.contains('{') && value.contains(':') && value.contains('}') {return "[redacted structured payload]".into();}
    let mut value=bounded(value,8192);
    if let Some(home)=home {
        value=replace_case(value,home.trim_end_matches(['\\','/']));
        value=replace_case(value,&home.replace('\\',"/").trim_end_matches('/'));
    }
    // Paths from another user's machine can appear in exception chains too.
    for pattern in [":\\users\\",":/users/","/home/"] {
        loop {
            let lower=value.to_ascii_lowercase();
            let Some(at)=lower.find(pattern) else {break};
            let start=if pattern.starts_with(':') && at>0 && value.as_bytes()[at-1].is_ascii_alphabetic() {at-1}else{at};
            let rest=at+pattern.len();
            let end=value[rest..].find(['\\','/','\r','\n','"','\'']).map_or(value.len(),|n|rest+n);
            value.replace_range(start..end,"[home]");
        }
    }
    let mut out=String::new();let mut chars=value.char_indices().peekable();
    while let Some((at,c))=chars.next() {
        if c=='0' && value[at..].starts_with("0x") || c=='0' && value[at..].starts_with("0X") {
            let end=at+2+value[at+2..].bytes().take_while(u8::is_ascii_hexdigit).count();
            if end>at+2 {
                out.push_str("[address]");while chars.peek().is_some_and(|(i,_)|*i<end){chars.next();}continue;
            }
        }
        if c.is_ascii_hexdigit() && (at==0 || !value.as_bytes()[at-1].is_ascii_alphanumeric()) {
            let len=value[at..].bytes().take_while(u8::is_ascii_hexdigit).count();
            if len>=9 && (at+len==value.len() || !value.as_bytes()[at+len].is_ascii_alphanumeric()) {
                out.push_str("[identifier]");while chars.peek().is_some_and(|(i,_)|*i<at+len){chars.next();}continue;
            }
        }
        out.push(if c.is_control() {' '} else {c});
    }
    bounded(&out,2048)
}

fn log_name(session:&str,part:u32)->String {format!("engine-{session}-{part:04}.jsonl")}
fn owned_name(name:&str)->Option<(&str,u32)> {
    let rest=name.strip_prefix("engine-")?.strip_suffix(".jsonl")?;
    let (session,part)=rest.rsplit_once('-')?;
    if session.len()!=32 || !session.bytes().all(|b|b.is_ascii_hexdigit()) || part.len()!=4
        || !part.bytes().all(|b|b.is_ascii_digit()) {return None;}
    Some((session,part.parse().ok()?))
}
struct Logger {
    directory:PathBuf,session:String,part:u32,file:Option<File>,bytes:u64,
    active:Option<File>,part_limit:u64,total_limit:u64,
    dedup:HashMap<String,String>,
}
impl Logger {
    fn open(directory:PathBuf,session:String,part_limit:u64,total_limit:u64)->io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let marker=directory.join(format!("engine-{session}.active"));
        // A live marker cannot be deleted by another editor/cleanup instance.
        let active=OpenOptions::new().write(true).create_new(true).share_mode(1).open(marker)?;
        let mut logger=Self {directory,session,part:0,file:None,bytes:0,active:Some(active),
            part_limit,total_limit,dedup:HashMap::new()};
        logger.rotate()?;Ok(logger)
    }
    fn rotate(&mut self)->io::Result<()> {
        self.file.take();
        if self.part>=9999 {return Err(io::Error::other("diagnostic part limit"));}
        self.part+=1;
        self.file=Some(OpenOptions::new().write(true).create_new(true).share_mode(1|2)
            .open(self.directory.join(log_name(&self.session,self.part)))?);
        self.bytes=0;self.cleanup();Ok(())
    }
    fn session_active(&self,session:&str)->bool {
        if session==self.session {return true;}
        let marker=self.directory.join(format!("engine-{session}.active"));
        if !marker.exists(){return false;}
        // Exclusively opening a stale marker succeeds; opening a live marker
        // is denied. Do not guess that access-denied markers are stale.
        match OpenOptions::new().write(true).share_mode(0).open(&marker) {
            Ok(handle)=>{drop(handle);let _=fs::remove_file(marker);false},Err(_)=>true,
        }
    }
    fn cleanup(&self) {
        let Ok(entries)=fs::read_dir(&self.directory) else{return};
        let mut files=Vec::new();let mut total=0;
        for entry in entries.flatten() {
            let name=entry.file_name().to_string_lossy().into_owned();
            let Some((session,part))=owned_name(&name) else{continue};
            let Ok(meta)=entry.metadata() else{continue};
            if !entry.file_type().is_ok_and(|kind|kind.is_file()){continue;}
            total+=meta.len();
            let is_own=session==self.session;
            let protected=if is_own {part==self.part}else{self.session_active(session)};
            let modified=meta.modified().unwrap_or(UNIX_EPOCH);
            let expired=SystemTime::now().duration_since(modified).unwrap_or_default().as_secs()>KEEP_DAYS;
            let old_part=is_own && part+KEEP_PARTS<=self.part;
            files.push((modified,entry.path(),meta.len(),protected,expired||old_part));
        }
        files.sort_by_key(|row|row.0);
        for (_,path,len,protected,expired) in files {
            if !protected && (expired || total>self.total_limit) && fs::remove_file(path).is_ok(){total=total.saturating_sub(len);}
        }
    }
    fn write(&mut self,level:&str,category:&str,event:&str,message:&str,causes:&[String],dedup:bool) {
        let message=sanitize(message);
        let causes:Vec<Value>=causes.iter().take(8).map(|cause|json!({"type":"error","message":sanitize(cause)})).collect();
        let key=format!("{category}/{event}");
        let signature=format!("{level}:{message}:{causes:?}");
        if dedup && self.dedup.get(&key)==Some(&signature){return;}
        let record=json!({"timestamp":utc_timestamp(millis()),"version":env!("CARGO_PKG_VERSION"),
            "session":self.session,"source":"engine","level":level,"category":category,
            "event":event,"message":message,"causes":causes});
        let Ok(mut bytes)=serde_json::to_vec(&record) else{return};bytes.push(b'\n');
        if self.bytes+bytes.len() as u64>self.part_limit && self.bytes>0 && self.rotate().is_err(){return;}
        if let Some(file)=&mut self.file {
            if file.write_all(&bytes).and_then(|_|file.flush()).is_ok() {
                self.bytes+=bytes.len() as u64;
                if dedup {
                    if self.dedup.len()>=64 && !self.dedup.contains_key(&key){self.dedup.clear();}
                    self.dedup.insert(key,signature);
                }
            }
        }
    }
}
impl Drop for Logger {
    fn drop(&mut self){self.file.take();self.active.take();let _=fs::remove_file(self.directory.join(format!("engine-{}.active",self.session)));}
}

pub(crate) fn init() {
    let Some(root)=std::env::var_os("LOCALAPPDATA") else{return};
    let directory=PathBuf::from(root).join("TouchlineEditor").join("Logs");
    let _=LOG.set(Mutex::new(Logger::open(directory,session_id(),PART_BYTES,SOURCE_BYTES).ok()));
    let previous=std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text=info.payload().downcast_ref::<String>().map(String::as_str)
            .or_else(||info.payload().downcast_ref::<&str>().copied()).unwrap_or("non-text panic");
        emit("error","lifecycle","panic","Engine panicked",&[text.to_owned()],false);
        previous(info);
    }));
    event("info","lifecycle","start","Engine started");
}
pub(crate) fn finish(success:bool) {
    event(if success {"info"}else{"error"},"lifecycle","end",if success {"Engine stopped"}else{"Engine stopped after failure"});
    if let Some(log)=LOG.get(){if let Ok(mut logger)=log.try_lock(){logger.take();}}
}
fn emit(level:&str,category:&str,event:&str,message:&str,causes:&[String],dedup:bool) {
    // Logging failure, poison or reentrancy must never affect the operation.
    if let Some(log)=LOG.get(){if let Ok(mut guard)=log.try_lock(){if let Some(logger)=guard.as_mut(){logger.write(level,category,event,message,causes,dedup);}}}
}
pub(crate) fn event(level:&str,category:&str,event:&str,message:&str){emit(level,category,event,message,&[],false);}
pub(crate) fn error(category:&str,event:&str,message:&str,error:&anyhow::Error) {
    let causes=if category=="command" {command_causes(error)}else{error.chain().map(ToString::to_string).collect::<Vec<_>>()};
    emit("error",category,event,message,&causes,true);
}
fn command_causes(error:&anyhow::Error)->Vec<String> {
    // Validation messages may quote arbitrary field values or search text.
    // Operational OS failures are safe after redaction; other command details
    // remain exclusively in the response shown by the app, never the log.
    error.chain().filter_map(|cause|cause.downcast_ref::<io::Error>().map(ToString::to_string)).collect()
}
pub(crate) fn connection(state:&str,reason:&str) {
    let message=if reason.is_empty(){state.to_owned()}else{format!("{state}: {reason}")};
    emit(if matches!(state,"error"|"unsupported_build"){"error"}else{"info"},"connection","state_changed",&message,&[],true);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory()->PathBuf {std::env::temp_dir().join(format!("touchline-log-test-{}",session_id()))}
    fn read_logs(dir:&Path)->String {fs::read_dir(dir).unwrap().flatten().filter(|e|e.file_type().is_ok_and(|kind|kind.is_file())&&owned_name(&e.file_name().to_string_lossy()).is_some())
        .map(|e|fs::read_to_string(e.path()).unwrap()).collect::<String>()}
    #[test]
    fn redacts_home_addresses_credentials_before_serializing_and_retains_context() {
        let text=sanitize_with_home("ReadProcessMemory C:\\Users\\Alice\\test 0x7FFE12345678 7ffe12345678 failed: access denied",Some("c:\\users\\alice"));
        assert!(text.contains("ReadProcessMemory")&&text.contains("access denied"));
        for secret in ["Alice","0x7FFE12345678","7ffe12345678"]{assert!(!text.contains(secret));}
        assert_eq!(sanitize("Authorization: Bearer very-private-token"),"[redacted credential-bearing text]");
        assert!(!sanitize("{\"token\":\"abcSECRET\"}").contains("abcSECRET"));
        assert!(!sanitize_with_home("open D:/Users/Bob/test",None).contains("Bob"));
        assert!(!sanitize_with_home("한:\\users\\Bob/test",None).contains("Bob"));
        assert_eq!(utc_timestamp(0),"1970-01-01T00:00:00.000Z");
        assert_eq!(utc_timestamp(1709210096789),"2024-02-29T12:34:56.789Z");
    }
    #[test]
    fn files_are_bounded_owned_and_other_active_sessions_are_preserved() {
        let dir=directory();let mut first=Logger::open(dir.clone(),"a".repeat(32),600,2400).unwrap();
        first.write("info","connection","state_changed","active",&[],true);
        fs::write(dir.join("unrelated.jsonl"),b"never delete").unwrap();
        let active_path=dir.join(log_name(&first.session,1));
        let mut second=Logger::open(dir.clone(),"b".repeat(32),600,2400).unwrap();
        for n in 0..20 {second.write("error","command","detail",&format!("failure {n}"),&[],false);}
        assert!(active_path.exists());assert!(dir.join("unrelated.jsonl").exists());
        let parts=fs::read_dir(&dir).unwrap().flatten().filter(|e|e.file_name().to_string_lossy().starts_with("engine-b")&&e.path().extension().is_some_and(|s|s=="jsonl")).count();
        assert!(parts<=KEEP_PARTS as usize);
        drop(first);drop(second);fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn dedup_stores_changes_and_failure_does_not_panic_or_change_operation() {
        let dir=directory();let mut logger=Logger::open(dir.clone(),"c".repeat(32),4096,8192).unwrap();
        for _ in 0..50 {logger.write("error","connection","state_changed","denied at 0x1122334455",&[],true);}
        logger.write("info","connection","state_changed","connected",&[],true);
        logger.write("error","connection","state_changed","denied at 0x1122334455",&[],true);
        let text=read_logs(&dir);assert_eq!(text.lines().count(),3);assert!(!text.contains("0x1122334455"));
        for line in text.lines(){let record:Value=serde_json::from_str(line).unwrap();assert_eq!(record["source"],"engine");assert!(record.get("args").is_none());assert!(record.get("result").is_none());}
        drop(logger);let blocked=dir.join("blocked");fs::write(&blocked,b"file").unwrap();
        assert!(Logger::open(blocked,"d".repeat(32),4096,8192).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn command_validation_never_records_arbitrary_arguments_or_names() {
        let error=anyhow::anyhow!("Unknown player private-player-name, supplied value secret-free-form");
        assert!(command_causes(&error).is_empty());
        let io=anyhow::Error::new(io::Error::from_raw_os_error(5)).context("query private-player-name");
        let causes=command_causes(&io);assert_eq!(causes.len(),1);assert!(!causes[0].contains("private-player-name"));
        assert_eq!(sanitize("unexpected {\"player\":\"private-player-name\"}"),"[redacted structured payload]");
    }
    #[test]
    fn rotation_denied_io_is_nonfatal_and_session_collision_never_clobbers() {
        let dir=directory();let session="e".repeat(32);
        let mut logger=Logger::open(dir.clone(),session.clone(),400,1600).unwrap();
        assert!(Logger::open(dir.clone(),session.clone(),400,1600).is_err());
        logger.write("info","lifecycle","start","first",&[],false);
        let before=read_logs(&dir);
        fs::create_dir(dir.join(log_name(&session,2))).unwrap();
        logger.write("error","connection","state_changed",&"failure".repeat(100),&[],false);
        assert_eq!(read_logs(&dir),before);
        drop(logger);fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn cleanup_removes_expired_closed_parts_only() {
        let dir=directory();fs::create_dir_all(&dir).unwrap();
        let old=dir.join(log_name(&"f".repeat(32),1));
        let file=File::create(&old).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(UNIX_EPOCH)).unwrap();drop(file);
        let logger=Logger::open(dir.clone(),"a".repeat(32),4096,8192).unwrap();
        assert!(!old.exists());drop(logger);fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn only_exact_owned_names_qualify_for_cleanup() {
        assert!(owned_name(&log_name(&"a".repeat(32),1)).is_some());
        for name in ["engine-foo.jsonl","client-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0001.jsonl","engine-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.active","engine-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0001.jsonl.bak"] {assert!(owned_name(name).is_none());}
    }
}
