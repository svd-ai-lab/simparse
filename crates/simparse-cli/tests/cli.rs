use std::process::Command;

use tempfile::tempdir;

#[test]
fn cli_reports_release_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_simparse"))
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("simparse ", env!("CARGO_PKG_VERSION"))
    );
}

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

#[test]
fn inspect_cli_summary_is_bounded_and_describes_limits() {
    let tmp = tempdir().unwrap();
    let deck = tmp.path().join("large.inp");
    let materials = (0..20)
        .map(|index| format!("*MATERIAL, NAME=MAT_{index}\n*ELASTIC\n1., 0.3\n"))
        .collect::<String>();
    std::fs::write(&deck, format!("*HEADING\nSummary case\n{materials}")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_simparse"))
        .arg("inspect")
        .arg(&deck)
        .arg("--json")
        .arg("--summary")
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["view"], "summary");
    assert_eq!(value["summary"]["data"]["materials"]["total"], 20);
    assert_eq!(
        value["summary"]["data"]["materials"]["sample"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(value["summary"]["data"]["materials"]["truncated"], true);
    assert!(
        value["limitations"][0]
            .as_str()
            .unwrap()
            .contains("Keyword inventory")
    );
    assert!(output.stdout.len() < 16 * 1024);
}
