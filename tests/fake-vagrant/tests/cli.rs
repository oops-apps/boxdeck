use std::process::Command;

#[test]
fn reports_a_vagrant_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_vagrant"))
        .arg("--version")
        .output()
        .expect("run fake vagrant");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Vagrant "));
}
