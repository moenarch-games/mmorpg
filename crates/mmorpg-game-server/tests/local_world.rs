//! One real-process lifecycle path: launch from a disposable client process, reuse,
//! reject a second writer, stop with durable recovery, and resume the same world.
use mmorpg_game_server::local_world;
use std::{path::Path, process::Command};

struct SavedWorld(tempfile::TempDir);
impl Drop for SavedWorld {
    fn drop(&mut self) {
        let _ = local_world::stop(self.0.path());
    }
}

#[test]
#[ignore = "helper process invoked by managed_world_survives_client_exit_and_resumes"]
fn launcher_process() {
    let directory = std::env::var_os("MMORPG_TEST_SAVE").unwrap();
    local_world::start(
        Path::new(&directory),
        Path::new(env!("CARGO_BIN_EXE_mmorpg-zone-host")),
    )
    .unwrap();
}

#[test]
fn managed_world_survives_client_exit_and_resumes() {
    let saved = SavedWorld(tempfile::tempdir().unwrap());
    let directory = saved.0.path();
    // The launching process exits completely. Its authoritative host must remain ready.
    let launched = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "launcher_process"])
        .env("MMORPG_TEST_SAVE", directory)
        .env("MMORPG_TEST_OFFLINE_HOST", "1")
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    let running = local_world::running(directory).unwrap().unwrap();
    let reused = local_world::start(directory, Path::new("unused-when-already-running")).unwrap();
    assert_eq!(running.url(), reused.url());
    assert_eq!(
        std::fs::read(&running.certificate).unwrap(),
        std::fs::read(&reused.certificate).unwrap()
    );
    let duplicate = Command::new(env!("CARGO_BIN_EXE_mmorpg-zone-host"))
        .arg("--local-world")
        .arg(directory)
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already running"));
    assert!(local_world::running(directory).unwrap().is_some());
    local_world::stop(directory).unwrap();
    assert!(local_world::running(directory).unwrap().is_none());
    assert!(!running.certificate.exists());
    let recovery = std::fs::read_dir(directory.join("recovery"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "recovery"))
        .unwrap();
    let before = game_server::RecoveryImage::read_file(&recovery)
        .unwrap()
        .current_tick;
    assert!(before > 0);
    let restarted = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "launcher_process"])
        .env("MMORPG_TEST_SAVE", directory)
        .env("MMORPG_TEST_OFFLINE_HOST", "1")
        .output()
        .unwrap();
    assert!(restarted.status.success());
    local_world::stop(directory).unwrap();
    let after = game_server::RecoveryImage::read_file(&recovery)
        .unwrap()
        .current_tick;
    assert!(after >= before, "saved world must continue, never reset");
    // Idempotent stop and no automatic restart.
    local_world::stop(directory).unwrap();
    assert!(local_world::running(directory).unwrap().is_none());
}
