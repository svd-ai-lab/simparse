use std::process::Command;

use tempfile::tempdir;

#[test]
fn inspect_cli_emits_json_without_paths_by_default() {
    let tmp = tempdir().unwrap();
    let deck = tmp.path().join("case.inp");
    std::fs::write(&deck, "*HEADING\nCLI case\n*NODE\n1,0,0,0\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_simparse"))
        .arg("inspect")
        .arg(&deck)
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"file_name\": \"case.inp\""));
    assert!(!stdout.contains("C:\\Users\\"));
    assert!(!stdout.contains(tmp.path().to_string_lossy().as_ref()));
}
