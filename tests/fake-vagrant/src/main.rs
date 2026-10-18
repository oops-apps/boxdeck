//! Scriptable fake `vagrant` for Boxdeck's integration and e2e tests.
//!
//! With no `FAKE_VAGRANT_SCRIPT`, `--version` prints a version and anything else fails.
//! Otherwise the script file runs one command per line:
//!
//! - `out TEXT` / `err TEXT`: print a line to stdout / stderr
//! - `fixture PATH`: copy a file to stdout
//! - `sleep MS`: sleep; an interrupt (Ctrl-C / SIGINT) during it prints a cleanup line and
//!   exits 130, like Vagrant's own handler
//! - `ignore-interrupt`: from now on, interrupts are ignored (needs a force kill)
//! - `read-stdin`: read stdin; at EOF, fail like Vagrant's non-interactive UI does
//! - `spawn-child PIDFILE`: start a long-running copy of itself and write its pid to PIDFILE
//! - `args`: print the received argv (after the program name) on one line, space-separated
//! - `env NAME`: print `NAME=value` (empty when unset)
//! - `exit CODE`: exit with CODE
//!
//! Lines starting with `#` and empty lines are skipped. The script ends with exit 0.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(script) = std::env::var_os("FAKE_VAGRANT_SCRIPT") else {
        if args.first().map(String::as_str) == Some("--version") {
            println!("Vagrant 2.4.9");
            return;
        }
        eprintln!("fake-vagrant: set FAKE_VAGRANT_SCRIPT");
        std::process::exit(1);
    };
    let text = std::fs::read_to_string(&script).expect("read FAKE_VAGRANT_SCRIPT");
    std::process::exit(run(&text, &args).await);
}

async fn run(script: &str, args: &[String]) -> i32 {
    let mut ignore_interrupt = false;
    for line in script.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        match cmd {
            "out" => println!("{rest}"),
            "err" => eprintln!("{rest}"),
            "fixture" => {
                let data = std::fs::read(rest).expect("read fixture");
                std::io::stdout().write_all(&data).expect("write fixture");
            }
            "sleep" => {
                let ms: u64 = rest.parse().expect("sleep MS");
                if sleep_or_interrupt(ms, ignore_interrupt).await {
                    println!("==> Waiting for cleanup before exiting...");
                    return 130;
                }
            }
            "ignore-interrupt" => ignore_interrupt = true,
            "read-stdin" => {
                let mut buf = String::new();
                let n = std::io::stdin().lock().read_line(&mut buf).unwrap_or(0);
                if n == 0 {
                    eprintln!(
                        "Vagrant is attempting to interface with the UI in a way that requires\na TTY."
                    );
                    return 1;
                }
                println!("got input: {}", buf.trim_end());
            }
            "spawn-child" => spawn_child(Path::new(rest)),
            "args" => println!("{}", args.join(" ")),
            "env" => println!("{rest}={}", std::env::var(rest).unwrap_or_default()),
            "exit" => return rest.parse().expect("exit CODE"),
            other => panic!("fake-vagrant: unknown command {other:?}"),
        }
        std::io::stdout().flush().ok();
    }
    0
}

/// Sleeps `ms`; returns true if an interrupt arrived first (and isn't being ignored).
async fn sleep_or_interrupt(ms: u64, ignore_interrupt: bool) -> bool {
    let sleep = tokio::time::sleep(Duration::from_millis(ms));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return false,
            r = tokio::signal::ctrl_c() => {
                r.expect("listen for interrupt");
                if !ignore_interrupt {
                    return true;
                }
            }
        }
    }
}

fn spawn_child(pidfile: &Path) {
    let dir = pidfile.parent().expect("pidfile has a parent");
    let child_script = dir.join("child.script");
    std::fs::write(&child_script, "ignore-interrupt\nsleep 60000\n").expect("write child script");
    let mut child = std::process::Command::new(std::env::current_exe().expect("current exe"))
        .env("FAKE_VAGRANT_SCRIPT", &child_script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn child");
    std::fs::write(pidfile, child.id().to_string()).expect("write pidfile");
    // Reaps the child whenever it exits, so it never lingers as a zombie.
    std::thread::spawn(move || child.wait());
}
