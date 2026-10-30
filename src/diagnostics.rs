//! Native I/O verification against our own isolated child, never against FM.
use crate::memory::Process;
use anyhow::{Context, Result, ensure};
use std::{
    io::{BufRead, Write},
    process::{Child, Command, Stdio},
    time::Duration,
};

pub fn fixture() -> Result<()> {
    let bytes = Box::new([11u8, 22, 33, 44, 55, 66, 77, 88]);
    println!("{} {}", std::process::id(), bytes.as_ptr() as usize);
    std::io::stdout().flush()?;
    loop {
        std::hint::black_box(&bytes);
        std::thread::sleep(Duration::from_secs(1));
    }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub fn memory_io() -> Result<serde_json::Value> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--memory-fixture")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = ChildGuard(cmd.spawn()?);
    let mut line = String::new();
    std::io::BufReader::new(child.0.stdout.take().context("fixture stdout")?)
        .read_line(&mut line)?;
    let args = line.split_whitespace().collect::<Vec<_>>();
    ensure!(args.len() == 2, "fixture protocol mismatch");
    let pid: u32 = args[0].parse()?;
    let address: u64 = args[1].parse()?;
    ensure!(pid == child.0.id(), "fixture PID mismatch");
    let original = vec![11, 22, 33, 44, 55, 66, 77, 88];
    let readonly = Process::open(pid, false)?;
    ensure!(
        readonly.read(address, 8)? == original,
        "initial read mismatch"
    );
    ensure!(
        readonly.write(address, &[99]).is_err(),
        "readonly write was not blocked"
    );
    let writer = Process::open(pid, true)?;
    let freeze = writer.suspend()?;
    let result = (|| -> Result<()> {
        writer.write(address, &[91, 82, 73, 64, 55, 46, 37, 28])?;
        ensure!(
            writer.read(address, 8)? == [91, 82, 73, 64, 55, 46, 37, 28],
            "write readback mismatch"
        );
        writer.write(address, &original)?;
        ensure!(
            writer.read(address, 8)? == original,
            "restore readback mismatch"
        );
        Ok(())
    })();
    freeze.resume()?;
    result?;
    Ok(
        serde_json::json!({"target":"isolated editor fixture process","fm_accessed":false,"read_only_handle_blocks_write":true,"write_readback":true,"restore_readback":true,"suspend_resume":true,"result":"passed"}),
    )
}
