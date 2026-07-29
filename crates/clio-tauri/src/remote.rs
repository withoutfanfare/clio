use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, RawContent};
use rmcp::transport::TokioChildProcess;
use rmcp::{Peer, RoleClient, ServiceExt};
use serde::de::DeserializeOwned;

use crate::CommandError;

const REMOTE_HOST_ENV: &str = "CLIO_REMOTE_HOST";
const REMOTE_DB_ENV: &str = "CLIO_REMOTE_DB_PATH";
const REMOTE_BINARY_ENV: &str = "CLIO_REMOTE_BINARY";
const REMOTE_COMMAND_ENV: &str = "CLIO_REMOTE_COMMAND";
#[cfg(not(test))]
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(test))]
const REMOTE_OPERATION_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(test)]
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
#[cfg(test)]
const REMOTE_OPERATION_TIMEOUT: Duration = Duration::from_secs(1);

pub struct RemoteConfig {
    pub host: String,
    pub db_path: String,
    pub remote_binary: String,
    pub command: String,
}

impl RemoteConfig {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            host: required_env(REMOTE_HOST_ENV)?,
            db_path: required_env(REMOTE_DB_ENV)?,
            remote_binary: required_env(REMOTE_BINARY_ENV)?,
            command: std::env::var(REMOTE_COMMAND_ENV).unwrap_or_else(|_| "clio".into()),
        })
    }

    pub fn from_settings(config: &clio_core::settings::RemoteConfig) -> Result<Self, String> {
        config.validate().map_err(|error| error.to_string())?;
        Ok(Self {
            host: config.host.clone(),
            db_path: config.db_path.clone(),
            remote_binary: config.mcp_binary.clone(),
            command: config.bridge_command.clone(),
        })
    }
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required when {REMOTE_HOST_ENV} enables remote mode"))
}

pub struct RemoteState {
    host: String,
    db_path: String,
    peer: Option<Peer<RoleClient>>,
    connected: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
    shutdown: Mutex<Option<rmcp::service::RunningServiceCancellationToken>>,
}

impl RemoteState {
    pub async fn connect(config: RemoteConfig) -> Self {
        let host = config.host.clone();
        let db_path = config.db_path.clone();
        let connected = Arc::new(AtomicBool::new(false));
        let last_error = Arc::new(Mutex::new(None));

        let mut command = tokio::process::Command::new(&config.command);
        command.args([
            "--db-path",
            &config.db_path,
            "remote-mcp",
            &config.host,
            "--remote-binary",
            &config.remote_binary,
        ]);

        let result = tokio::time::timeout(REMOTE_CONNECT_TIMEOUT, async {
            let transport = TokioChildProcess::new(command)
                .map_err(|e| format!("failed to start {}: {e}", config.command))?;
            let service = ().serve(transport).await.map_err(|e| e.to_string())?;
            let peer = service.peer().clone();
            let shutdown = service.cancellation_token();
            Ok::<_, String>((service, peer, shutdown))
        })
        .await
        .map_err(|_| timeout_error("bridge connection", REMOTE_CONNECT_TIMEOUT))
        .and_then(|result| result);

        match result {
            Ok((service, peer, shutdown)) => {
                connected.store(true, Ordering::Release);
                let connection_flag = connected.clone();
                let connection_error = last_error.clone();
                tauri::async_runtime::spawn(async move {
                    let reason = service.waiting().await;
                    connection_flag.store(false, Ordering::Release);
                    if let Ok(mut error) = connection_error.lock() {
                        *error = Some(format!("Atlas bridge stopped: {reason:?}"));
                    }
                });
                Self {
                    host,
                    db_path,
                    peer: Some(peer),
                    connected,
                    last_error,
                    shutdown: Mutex::new(Some(shutdown)),
                }
            }
            Err(error) => Self::disconnected(host, db_path, error),
        }
    }

    pub fn disconnected(host: String, db_path: String, error: String) -> Self {
        Self {
            host,
            db_path,
            peer: None,
            connected: Arc::new(AtomicBool::new(false)),
            last_error: Arc::new(Mutex::new(Some(error))),
            shutdown: Mutex::new(None),
        }
    }

    pub async fn call_json<T: DeserializeOwned>(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<T, CommandError> {
        let peer = self.peer.as_ref().ok_or_else(|| {
            CommandError::Config(
                self.last_error()
                    .unwrap_or_else(|| "Atlas bridge is not connected".into()),
            )
        })?;
        let arguments = arguments.as_object().cloned().ok_or_else(|| {
            CommandError::Config("Remote MCP arguments must be a JSON object".into())
        })?;

        tracing::debug!(target: "clio_tauri_remote", tool, host = self.host, "calling Atlas MCP tool");
        let request = peer.call_tool(CallToolRequestParams {
            meta: None,
            name: Cow::Owned(tool.to_string()),
            arguments: Some(arguments),
            task: None,
        });
        let result = match tokio::time::timeout(REMOTE_OPERATION_TIMEOUT, request).await {
            Ok(result) => result.map_err(|error| {
                self.record_error(error.to_string());
                CommandError::Core(format!("Atlas bridge error: {error}"))
            })?,
            Err(_) => {
                let mut error =
                    timeout_error(&format!("MCP tool {tool}"), REMOTE_OPERATION_TIMEOUT);
                if is_mutating_tool(tool) {
                    error.push_str("; its outcome is unknown, so check the memory before retrying");
                }
                self.record_error(error.clone());
                self.shutdown();
                return Err(CommandError::Core(error));
            }
        };

        decode_tool_result(result)
    }

    pub async fn status(&self) -> ConnectionStatus {
        // Status is transport state, not a health probe. Sending a tool call
        // here would queue behind a long capture and could cancel an in-flight
        // write when the probe times out.
        let connected = self.connected.load(Ordering::Acquire);

        ConnectionStatus {
            backend: "remote".into(),
            label: self.host.clone(),
            connected,
            detail: if connected {
                Some(self.db_path.clone())
            } else {
                self.last_error()
            },
        }
    }

    pub fn shutdown(&self) {
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(token) = shutdown.take() {
                token.cancel();
            }
        }
        self.connected.store(false, Ordering::Release);
    }

    fn record_error(&self, error: String) {
        self.connected.store(false, Ordering::Release);
        if let Ok(mut last_error) = self.last_error.lock() {
            *last_error = Some(error);
        }
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|error| error.clone())
    }
}

impl Drop for RemoteState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone, serde::Serialize)]
pub struct ConnectionStatus {
    pub backend: String,
    pub label: String,
    pub connected: bool,
    pub detail: Option<String>,
}

fn timeout_error(operation: &str, timeout: Duration) -> String {
    format!(
        "Atlas {operation} timed out after {} seconds",
        timeout.as_secs()
    )
}

fn is_mutating_tool(tool: &str) -> bool {
    matches!(
        tool,
        "memory_remember"
            | "memory_update"
            | "memory_link"
            | "memory_archive"
            | "memory_unarchive"
            | "memory_delete"
            | "memory_move"
            | "memory_capture"
            | "memory_inbox"
            | "memory_cache_clear"
    )
}

fn decode_tool_result<T: DeserializeOwned>(result: CallToolResult) -> Result<T, CommandError> {
    let text = result
        .content
        .into_iter()
        .find_map(|content| match content.raw {
            RawContent::Text(text) => Some(text.text),
            _ => None,
        })
        .ok_or_else(|| CommandError::Core("Atlas MCP returned no text result".into()))?;

    if result.is_error == Some(true) {
        return Err(CommandError::Core(text));
    }

    serde_json::from_str(&text).map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::process::{Command, Stdio};
    #[cfg(unix)]
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use rmcp::model::{CallToolResult, Content};

    use super::{RemoteConfig, RemoteState, decode_tool_result, is_mutating_tool};

    #[test]
    fn classifies_unknown_outcome_tool_timeouts() {
        assert!(is_mutating_tool("memory_remember"));
        assert!(is_mutating_tool("memory_capture"));
        assert!(!is_mutating_tool("memory_recall"));
        assert!(!is_mutating_tool("memory_stats"));
    }

    #[test]
    fn decodes_json_and_surfaces_tool_errors() {
        let value: serde_json::Value =
            decode_tool_result(CallToolResult::success(vec![Content::text(
                r#"{"connected":true}"#,
            )]))
            .unwrap();
        assert_eq!(value["connected"], true);

        let error =
            decode_tool_result::<serde_json::Value>(CallToolResult::error(vec![Content::text(
                "remote failure",
            )]))
            .unwrap_err();
        assert_eq!(error.to_string(), "remote failure");
    }

    #[test]
    fn builds_remote_config_from_persisted_settings() {
        let persisted = clio_core::settings::RemoteConfig {
            host: "atlas".into(),
            db_path: "/srv/memory.db".into(),
            mcp_binary: "/srv/clio-mcp".into(),
            cli_binary: "/srv/clio".into(),
            bridge_command: "/usr/local/bin/clio".into(),
        };
        let config = RemoteConfig::from_settings(&persisted).unwrap();
        assert_eq!(config.host, "atlas");
        assert_eq!(config.command, "/usr/local/bin/clio");
    }

    #[cfg(unix)]
    #[test]
    fn connection_timeout_stops_the_bridge_process() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("clio-timeout-{suffix}"));
        let script = directory.join("bridge");
        let pid_file = directory.join("pid");
        fs::create_dir(&directory).unwrap();
        fs::write(&script, "#!/bin/sh\necho $$ > \"$2\"\nexec sleep 30\n").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();

        let state = tauri::async_runtime::block_on(RemoteState::connect(RemoteConfig {
            host: "atlas".into(),
            db_path: pid_file.to_string_lossy().into_owned(),
            remote_binary: "unused".into(),
            command: script.to_string_lossy().into_owned(),
        }));
        assert!(state.last_error().unwrap().contains("timed out"));

        let pid = fs::read_to_string(&pid_file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let stopped = loop {
            let running = Command::new("kill")
                .args(["-0", pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !running || Instant::now() >= deadline {
                break !running;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if !stopped {
            let _ = Command::new("kill")
                .arg(pid.trim())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        fs::remove_dir_all(directory).unwrap();
        assert!(stopped, "timed-out bridge process was still running");
    }
}
