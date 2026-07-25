use std::borrow::Cow;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

pub(crate) fn run(
    host: &str,
    remote_binary: &str,
    remote_db: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new("ssh")
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--"])
        .arg(host)
        .arg(remote_command(remote_binary, remote_db))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("failed to start SSH bridge to {host}: {err}"))?;

    let mut remote_stdin = child.stdin.take().expect("SSH stdin should be piped");
    let mut remote_stdout = child.stdout.take().expect("SSH stdout should be piped");
    let input_thread = std::thread::spawn(move || {
        let stdin = io::stdin();
        forward_requests(stdin.lock(), &mut remote_stdin)
    });
    let output_thread = std::thread::spawn(move || {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        io::copy(&mut remote_stdout, &mut stdout)?;
        stdout.flush()
    });

    let status_result = child.wait();
    let output_result = output_thread
        .join()
        .map_err(|_| io::Error::other("SSH output forwarding thread panicked"))?;
    let input_result = if input_thread.is_finished() {
        Some(
            input_thread
                .join()
                .map_err(|_| io::Error::other("MCP input forwarding thread panicked"))?,
        )
    } else {
        None
    };

    let status = status_result?;
    if !status.success() {
        return Err(format!("SSH bridge to {host} exited with {status}").into());
    }
    output_result?;
    if let Some(input_result) = input_result {
        input_result?;
    }

    Ok(())
}

fn forward_requests<R: BufRead, W: Write>(mut reader: R, writer: &mut W) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        writer.write_all(&rewrite_request(&line))?;
        writer.flush()?;
    }
    Ok(())
}

fn rewrite_request(line: &[u8]) -> Cow<'_, [u8]> {
    let Ok(mut request) = serde_json::from_slice::<Value>(line) else {
        return Cow::Borrowed(line);
    };
    if request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return Cow::Borrowed(line);
    }

    let Some(arguments) = request
        .get_mut("params")
        .and_then(|params| params.get_mut("arguments"))
        .and_then(Value::as_object_mut)
    else {
        return Cow::Borrowed(line);
    };
    if arguments
        .get("namespace")
        .is_some_and(|namespace| !namespace.is_null())
        || arguments.get("global").and_then(Value::as_bool) == Some(true)
    {
        return Cow::Borrowed(line);
    }

    let Some(cwd) = arguments.get("cwd").and_then(Value::as_str) else {
        return Cow::Borrowed(line);
    };
    let detected = clio_core::context::detect_namespace(Path::new(cwd));

    arguments.remove("cwd");
    arguments.remove("_clio_namespace");
    if let Some(detected) = detected {
        arguments.insert(
            "_clio_namespace".to_string(),
            Value::String(detected.namespace),
        );
    }
    let mut rewritten = serde_json::to_vec(&request).expect("serialising JSON value cannot fail");
    if line.ends_with(b"\r\n") {
        rewritten.extend_from_slice(b"\r\n");
    } else if line.ends_with(b"\n") {
        rewritten.push(b'\n');
    }
    Cow::Owned(rewritten)
}

fn remote_command(remote_binary: &str, remote_db: Option<&str>) -> String {
    match remote_db {
        Some(remote_db) => format!(
            "env CLIO_DB_PATH={} {}",
            shell_quote(remote_db),
            shell_quote(remote_binary)
        ),
        None => shell_quote(remote_binary),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn namespace_dir() -> std::path::PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("clio-remote-mcp-{}-{unique}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::write(path.join(".clio-namespace"), "project:bridge-test\n").unwrap();
        path
    }

    #[test]
    fn injects_detected_namespace_when_public_namespace_is_absent_or_null() {
        let cwd = namespace_dir();
        for include_null_namespace in [false, true] {
            let mut request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": { "arguments": { "cwd": cwd.to_str().unwrap() } }
            });
            if include_null_namespace {
                request["params"]["arguments"]["namespace"] = Value::Null;
            }
            let mut line = serde_json::to_vec(&request).unwrap();
            line.extend_from_slice(b"\r\n");

            let rewritten = rewrite_request(&line);
            assert!(rewritten.ends_with(b"\r\n"));
            let value: Value = serde_json::from_slice(&rewritten).unwrap();
            let arguments = value["params"]["arguments"].as_object().unwrap();
            assert_eq!(arguments.contains_key("namespace"), include_null_namespace);
            assert!(!arguments.contains_key("cwd"));
            assert_eq!(arguments["_clio_namespace"], "project:bridge-test");
        }

        fs::remove_dir_all(cwd).unwrap();
    }

    #[test]
    fn leaves_explicit_global_and_non_tool_requests_untouched() {
        let cwd = namespace_dir();
        let cwd = serde_json::to_string(cwd.to_str().unwrap()).unwrap();
        let non_tool = serde_json::json!({
            "method": "initialize",
            "params": { "arguments": { "cwd": serde_json::from_str::<String>(&cwd).unwrap() } }
        })
        .to_string()
            + "\n";
        let lines = [
            format!(
                "{{ \"method\": \"tools/call\", \"params\": {{\"arguments\": {{\"cwd\":{cwd},\"namespace\":\"project:explicit\"}}}} }}\n"
            ),
            format!(
                "{{\"method\":\"tools/call\",\"params\":{{\"arguments\":{{\"cwd\":{cwd},\"global\":true}}}}}}\n"
            ),
            non_tool,
            "not json at all\n".to_string(),
        ];

        for line in lines {
            assert!(matches!(rewrite_request(line.as_bytes()), Cow::Borrowed(_)));
            assert_eq!(rewrite_request(line.as_bytes()).as_ref(), line.as_bytes());
        }

        fs::remove_dir_all(serde_json::from_str::<String>(&cwd).unwrap()).unwrap();
    }

    #[test]
    fn removes_undetected_local_cwd_before_forwarding() {
        let cwd = namespace_dir();
        fs::remove_file(cwd.join(".clio-namespace")).unwrap();
        let request = serde_json::json!({
            "method": "tools/call",
            "params": { "arguments": { "cwd": cwd.to_str().unwrap() } }
        });

        let line = request.to_string();
        let rewritten = rewrite_request(line.as_bytes());
        let value: Value = serde_json::from_slice(&rewritten).unwrap();
        let arguments = value["params"]["arguments"].as_object().unwrap();
        assert!(!arguments.contains_key("cwd"));
        assert!(!arguments.contains_key("_clio_namespace"));

        fs::remove_dir(cwd).unwrap();
    }

    #[test]
    fn quotes_remote_shell_arguments() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("/srv/clio's bin"), "'/srv/clio'\\''s bin'");
        assert_eq!(
            remote_command("/srv/clio's bin", Some("/srv/memory db")),
            "env CLIO_DB_PATH='/srv/memory db' '/srv/clio'\\''s bin'"
        );
        assert_eq!(remote_command("/srv/clio-mcp", None), "'/srv/clio-mcp'");
    }
}
