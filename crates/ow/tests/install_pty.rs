//! The consent prompt on a real pseudo-terminal. Text typed (or pasted) before the prompt is
//! shown must not answer it: `TtyUi::ask` discards the terminal's input queue first. The test
//! re-runs this test binary as the child that asks, so it needs no network.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]
use ow::install::{TtyUi, Ui};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CHILD: &str = "OW_PTY_CHILD";

/// Runs only inside the child process (as `--exact child_asks`): reads the "summary" for a
/// moment, then asks like `ow install` does.
#[test]
fn child_asks() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    std::thread::sleep(Duration::from_millis(700));
    let a = TtyUi.ask("PROMPT> ").unwrap();
    println!("ANSWER={a}");
}

fn wait_for(buf: &Arc<Mutex<Vec<u8>>>, needle: &str, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if String::from_utf8_lossy(&buf.lock().unwrap()).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn text_already_in_the_terminal_buffer_does_not_answer_the_prompt() {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name.to_str().unwrap())
        .unwrap();
    let mut master = std::fs::File::from(master);
    let out = Arc::new(Mutex::new(Vec::new()));
    let (o2, mut r) = (Arc::clone(&out), master.try_clone().unwrap());
    std::thread::spawn(move || {
        let mut b = [0u8; 256];
        while let Ok(n) = r.read(&mut b) {
            if n == 0 {
                break;
            }
            o2.lock().unwrap().extend_from_slice(&b[..n]);
        }
    });
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_asks", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn()
        .unwrap();
    // A pasted "ow install X\nyes\n": the second line is already queued when the prompt appears.
    master.write_all(b"yes\n").unwrap();
    assert!(wait_for(&out, "PROMPT> ", 10), "prompt never shown");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !String::from_utf8_lossy(&out.lock().unwrap()).contains("ANSWER="),
        "type-ahead answered the prompt: {:?}",
        String::from_utf8_lossy(&out.lock().unwrap())
    );
    // Typed after the prompt is shown, an answer is read as usual.
    master.write_all(b"no\n").unwrap();
    assert!(
        wait_for(&out, "ANSWER=no", 10),
        "the typed answer was not read"
    );
    assert!(child.wait().unwrap().success());
}
