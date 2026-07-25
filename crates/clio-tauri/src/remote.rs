use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, CallToolResult, RawContent};
use rmcp::transport::TokioChildProcess;
use rmcp::{Peer, RoleClient, ServiceExt};
use serde::de::DeserializeOwned;

use crate::CommandError;

const REMOTE_HOST_ENV: &str = "CLIO_REMOTE_HOST";
const REMOTE_DB_ENV: &str = "CLIO_REMOTE_DB_PATH";
const REMOTE_BINARY_ENV: &str = "CLIO_REMOTE_BINARY";
const REMOTE_COMMAND_ENV: &str = "CLIO_REMOTE_COMMAND";

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

        let result = async {
            let transport = TokioChildProcess::new(command)
                .map_err(|e| format!("failed to start {}: {e}", config.command))?;
            let service = ().serve(transport).await.map_err(|e| e.to_string())?;
            let peer = service.peer().clone();
            let shutdown = service.cancellation_token();
            Ok::<_, String>((service, peer, shutdown))
        }
        .await;

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
        let result = peer
            .call_tool(CallToolRequestParams {
                meta: None,
                name: Cow::Owned(tool.to_string()),
                arguments: Some(arguments),
                task: None,
            })
            .await
            .map_err(|error| {
                self.record_error(error.to_string());
                CommandError::Core(format!("Atlas bridge error: {error}"))
            })?;

        decode_tool_result(result)
    }

    pub async fn status(&self) -> ConnectionStatus {
        let connected = if let Some(peer) = self.peer.as_ref() {
            match peer.list_tools(None).await {
                Ok(_) => {
                    self.connected.store(true, Ordering::Release);
                    true
                }
                Err(error) => {
                    self.record_error(error.to_string());
                    false
                }
            }
        } else {
            false
        };

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
    use rmcp::model::{CallToolResult, Content};

    use super::decode_tool_result;

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
}
