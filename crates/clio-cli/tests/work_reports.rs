use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn invoke(db: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_clio"))
        .arg("--local")
        .arg("--db-path")
        .arg(db)
        .arg("--json")
        .arg("work")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        let _ = child.stdin.take().unwrap().write_all(input);
    }
    child.wait_with_output().unwrap()
}

fn output_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn sample() -> Value {
    json!({"project":"exact/project", "task":"task-1", "task_title":"Adapter proof",
        "source":"adapter-test", "session_id":"session-1", "run_id":"run-1",
        "sequence":0, "observed_at":1, "worktree":"/isolated/tree", "revision":"abc123",
        "state":"running", "summary":"Test observation", "next_step":"Check receipt",
        "next_actor":"agent", "evidence":[], "evidence_status":"current", "supersedes":null})
}

#[test]
fn work_cli_replays_rejects_conflicts_and_filters_exact_projects() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("proof.db");
    let mut report = sample();
    let bytes = serde_json::to_vec(&report).unwrap();
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let first = output_json(invoke(&db, &["report", "-"], Some(&bytes)));
    assert!(first["received_at"].as_u64().unwrap() >= before);
    assert_eq!(
        first,
        output_json(invoke(&db, &["report", "-"], Some(&bytes)))
    );
    report["summary"] = json!("Conflicting replay");
    let conflict = invoke(
        &db,
        &["report", "-"],
        Some(&serde_json::to_vec(&report).unwrap()),
    );
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("different payload"));
    let overview = output_json(invoke(
        &db,
        &["overview", "--project", "exact/project"],
        None,
    ));
    assert_eq!(overview["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(overview["tasks"][0]["state"], "reporting_missing");
    assert_eq!(
        output_json(invoke(&db, &["overview", "--project", "exact"], None))["tasks"],
        json!([])
    );
    assert!(
        !invoke(&db, &["overview", "--project", " "], None)
            .status
            .success()
    );
    assert!(
        !invoke(&db, &["overview", "--stale-after-secs=-1"], None)
            .status
            .success()
    );
    assert_eq!(
        output_json(invoke(
            &db,
            &["history", "--source", "adapter-test", "--run-id", "run-1"],
            None
        )),
        json!([first])
    );
}

#[test]
fn work_cli_rejects_untyped_unknown_and_oversized_reports() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("proof.db");
    let mut report = sample();
    report["authority"] = json!("accepted");
    let unknown = invoke(
        &db,
        &["report", "-"],
        Some(&serde_json::to_vec(&report).unwrap()),
    );
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown field"));
    assert!(!invoke(&db, &["report", "-"], Some(b"{}")).status.success());
    let oversized = invoke(&db, &["report", "-"], Some(&vec![b' '; 128 * 1024 + 1]));
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("128 KiB"));
}

#[cfg(unix)]
#[test]
fn work_cli_remote_failure_never_falls_back_and_explicit_overrides_stay_local() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("proof.db");
    let mut settings = clio_core::settings::Settings::default();
    settings.remote = Some(clio_core::settings::RemoteConfig {
        host: "proof-host".into(),
        db_path: "/proof/remote.db".into(),
        mcp_binary: "/proof/clio-mcp".into(),
        cli_binary: "/proof/clio".into(),
        bridge_command: "/proof/bridge".into(),
    });
    clio_core::settings::save(&db, &settings).unwrap();
    let ssh = directory.path().join("ssh");
    std::fs::write(&ssh, "#!/bin/sh\nprintf '%s\\n' \"$*\" >&2\nexit 47\n").unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let invoke = |overrides: &[&str], command: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_clio"))
            .env("CLIO_DB_PATH", &db)
            .env("PATH", directory.path())
            .args(overrides)
            .arg("work")
            .args(command)
            .output()
            .unwrap()
    };
    let report_file = directory.path().join("report.json");
    std::fs::write(&report_file, serde_json::to_vec(&sample()).unwrap()).unwrap();
    let filename = report_file.to_str().unwrap();
    let rejected = invoke(&[], &["report", filename]);
    let error = String::from_utf8_lossy(&rejected.stderr);
    assert!(!rejected.status.success());
    assert!(error.contains("clio work report - < local.json"), "{error}");
    assert!(
        !error.contains("proof-host"),
        "file-form report must never invoke SSH: {error}"
    );
    assert!(!db.exists());
    let stdin_report = invoke(&[], &["report", "-"]);
    assert!(!stdin_report.status.success());
    let error = String::from_utf8_lossy(&stdin_report.stderr);
    assert!(
        error.contains("proof-host") && error.contains("'work' 'report' '-'"),
        "{error}"
    );
    assert!(!db.exists());
    let remote = invoke(&[], &["overview"]);
    assert!(!remote.status.success());
    let error = String::from_utf8_lossy(&remote.stderr);
    assert!(
        error.contains("proof-host") && error.contains("'work' 'overview'"),
        "{error}"
    );
    assert!(
        !db.exists(),
        "failed remote invocation must not open local storage"
    );
    assert_eq!(
        output_json(invoke(&["--local"], &["overview"]))["tasks"],
        json!([])
    );
    assert_eq!(
        output_json(invoke(&["--local"], &["report", filename]))["report"],
        sample()
    );
    let explicit = directory.path().join("explicit.db");
    assert_eq!(
        output_json(invoke(
            &["--db-path", explicit.to_str().unwrap()],
            &["overview"]
        ))["tasks"],
        json!([])
    );
    assert!(explicit.exists());
    assert_eq!(
        output_json(invoke(
            &["--db-path", explicit.to_str().unwrap()],
            &["report", filename]
        ))["report"],
        sample()
    );
}

#[test]
fn local_guidance_round_trips_without_registering_recommended_work() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("guidance.db");
    let mut report = sample();
    report["state"] = json!("implemented");
    report["next_actor"] = json!("user");
    let receipt = output_json(invoke(
        &db,
        &["report", "-"],
        Some(&serde_json::to_vec(&report).unwrap()),
    ));
    let acceptance = json!({"receipt_id":receipt["id"],"scope":"Item 3 only","accepted_by":"Danny","accepted_at":2,"evidence":["checks.md#human-acceptance"]});
    assert_eq!(
        output_json(invoke(
            &db,
            &["accept", "-"],
            Some(&serde_json::to_vec(&acceptance).unwrap())
        )),
        acceptance
    );
    let recommendation = json!({"project":"exact/project","parent_task":"task-1","task":"freshness","task_title":"Show report age","reason":"Polling age hides stale reports","next_actor":"user","prompt":"Implement only the recorded freshness finding.","evidence":["checks.md#item-2"],"checked_at":3});
    assert_eq!(
        output_json(invoke(
            &db,
            &["recommend", "-"],
            Some(&serde_json::to_vec(&recommendation).unwrap())
        )),
        recommendation
    );
    let view = output_json(invoke(&db, &["overview"], None));
    assert_eq!(view["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(view["tasks"][0]["state"], "accepted");
    assert_eq!(view["tasks"][0]["next_actor"], "none");
    assert_eq!(view["tasks"][0]["recommendation"], recommendation);
    let rejected = Command::new(env!("CARGO_BIN_EXE_clio"))
        .args(["--db-path", db.to_str().unwrap(), "work", "accept", "-"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("require --local"));
}
