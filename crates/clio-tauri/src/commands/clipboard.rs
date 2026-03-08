use std::io::Write;
use std::process::{Command, Stdio};

use crate::CommandError;

/// Copy text to the macOS clipboard via osascript with pbcopy fallback.
///
/// Uses AppleScript's `set the clipboard to` (reading content from a temp
/// file to avoid escaping issues) for best clipboard manager compatibility
/// (e.g. Paste), falling back to pbcopy if that fails.
#[tauri::command]
pub fn cmd_copy_to_clipboard(text: String) -> Result<(), CommandError> {
    copy_via_osascript(&text).or_else(|_| copy_via_pbcopy(&text))
}

fn copy_via_osascript(text: &str) -> Result<(), CommandError> {
    // Write to a temp file to avoid AppleScript string escaping issues
    // with arbitrary markdown content (newlines, quotes, backslashes).
    let tmp = std::env::temp_dir().join(format!("clio-clip-{}.txt", std::process::id()));
    std::fs::write(&tmp, text)
        .map_err(|e| CommandError::Core(format!("Failed to write temp file: {e}")))?;

    let script = format!(
        "set the clipboard to (read POSIX file \"{}\" as «class utf8»)",
        tmp.display()
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    let _ = std::fs::remove_file(&tmp);

    let output =
        output.map_err(|e| CommandError::Core(format!("Failed to run osascript: {e}")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(CommandError::Core(format!("osascript failed: {stderr}")))
    }
}

fn copy_via_pbcopy(text: &str) -> Result<(), CommandError> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| CommandError::Core(format!("Failed to spawn pbcopy: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| CommandError::Core(format!("Failed to write to pbcopy: {e}")))?;
    }

    let status = child
        .wait()
        .map_err(|e| CommandError::Core(format!("pbcopy failed: {e}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(CommandError::Core("pbcopy exited with non-zero status".into()))
    }
}
