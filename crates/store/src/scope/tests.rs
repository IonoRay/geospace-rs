use super::*;
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
};
use tempfile::TempDir;

fn digest() -> Sha256Digest {
    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        .parse()
        .unwrap()
}

#[test]
fn browse_view_refuses_parent_symlink_and_regular_file() {
    let temporary = TempDir::new().unwrap();
    let home = crate::StoreHome::new(temporary.path().to_path_buf()).unwrap();
    let layout = StoreLayout::new(&home).scoped("dst");
    layout.initialize().unwrap();
    let objects = layout.objects();
    let root = objects.parent().unwrap();
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("evil")).unwrap();
    assert!(create_view(&layout, Path::new("evil/link"), digest()).is_err());
    fs::write(root.join("plain"), b"user").unwrap();
    assert!(create_view(&layout, Path::new("plain/link"), digest()).is_err());
}

#[test]
fn collision_suffix_is_deterministic() {
    assert_eq!(
        browse_collision(Path::new("host/now__file"), 2).unwrap(),
        PathBuf::from("host/now__file.2")
    );
}

#[test]
fn cross_process_file_locks_release_on_exit() {
    if let (Ok(path), Ok(mode)) = (
        std::env::var("IONORAY_LOCK_CHILD"),
        std::env::var("IONORAY_LOCK_MODE"),
    ) {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        if mode == "shared" {
            file.try_lock_shared().unwrap();
        } else {
            file.try_lock().unwrap();
        }
        println!("ready");
        std::io::stdout().flush().unwrap();
        let mut ignored = String::new();
        std::io::stdin().read_to_string(&mut ignored).unwrap();
        return;
    }
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("scope.lock");
    run_lock_child(&path, "exclusive", |file| {
        assert!(file.try_lock().is_err());
        assert!(file.try_lock_shared().is_err());
    });
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.try_lock().unwrap();
    file.unlock().unwrap();
    run_lock_child(&path, "shared", |file| {
        file.try_lock_shared().unwrap();
        assert!(file.try_lock().is_err());
        file.unlock().unwrap();
    });
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.try_lock().unwrap();
}

fn run_lock_child(path: &Path, mode: &str, check: impl FnOnce(&std::fs::File)) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "scope::tests::cross_process_file_locks_release_on_exit",
            "--nocapture",
        ])
        .env("IONORAY_LOCK_CHILD", path)
        .env("IONORAY_LOCK_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("ready") {
        line.clear();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    check(&file);
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}
