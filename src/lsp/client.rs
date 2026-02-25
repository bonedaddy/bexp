use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse,
    InitializeParams, Location, Position, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Response timeout for LSP requests.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// If no response received in this long, consider the process dead.
const PROCESS_HEALTH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct LspClient {
    process: Child,
    next_id: AtomicI64,
    workspace_root: String,
    response_rx: Receiver<JsonRpcMessage>,
    _reader_thread: Option<JoinHandle<()>>,
    opened_files: HashSet<String>,
    last_response_at: Arc<Mutex<Instant>>,
    /// Buffer for out-of-order responses received while waiting for a specific request ID.
    pending_responses: HashMap<i64, Option<Value>>,
}

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: i64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcMessage {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<i64>,
    result: Option<Value>,
    #[allow(dead_code)]
    error: Option<Value>,
    #[allow(dead_code)]
    method: Option<String>,
}

impl LspClient {
    pub fn spawn(command: &str, args: &[String], workspace_root: &str) -> std::io::Result<Self> {
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        // Take stdout and spawn a reader thread that pushes messages into a channel
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("No stdout available"))?;

        let (tx, rx): (Sender<JsonRpcMessage>, Receiver<JsonRpcMessage>) =
            crossbeam_channel::bounded(256);

        let reader_thread = std::thread::spawn(move || {
            Self::reader_loop(stdout, tx);
        });

        let last_response_at = Arc::new(Mutex::new(Instant::now()));

        Ok(Self {
            process,
            next_id: AtomicI64::new(1),
            workspace_root: workspace_root.to_string(),
            response_rx: rx,
            _reader_thread: Some(reader_thread),
            opened_files: HashSet::new(),
            last_response_at,
            pending_responses: HashMap::new(),
        })
    }

    /// Background reader loop: continuously reads LSP messages from stdout
    /// and sends them into the channel.
    fn reader_loop(stdout: std::process::ChildStdout, tx: Sender<JsonRpcMessage>) {
        let mut reader = BufReader::new(stdout);

        loop {
            // Read headers
            let mut content_length: Option<usize> = None;
            loop {
                let mut header = String::new();
                match reader.read_line(&mut header) {
                    Ok(0) => return, // EOF
                    Err(_) => return,
                    _ => {}
                }
                let header = header.trim();
                if header.is_empty() {
                    break;
                }
                if let Some(len_str) = header.strip_prefix("Content-Length: ") {
                    content_length = len_str.parse().ok();
                }
            }

            let content_length = match content_length {
                Some(len) => len,
                None => continue,
            };

            // Read body
            let mut body = vec![0u8; content_length];
            if std::io::Read::read_exact(&mut reader, &mut body).is_err() {
                return;
            }

            let message: JsonRpcMessage = match serde_json::from_slice(&body) {
                Ok(m) => m,
                Err(_) => continue,
            };

            if tx.send(message).is_err() {
                return; // Receiver dropped
            }
        }
    }

    pub fn initialize(&mut self) -> anyhow::Result<()> {
        let workspace_uri = Uri::from_str(&format!("file://{}", self.workspace_root))
            .map_err(|e| anyhow::anyhow!("Invalid workspace path: {}", e))?;

        let params = InitializeParams {
            capabilities: ClientCapabilities::default(),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: workspace_uri,
                name: "workspace".to_string(),
            }]),
            ..Default::default()
        };

        let _response = self.send_request("initialize", serde_json::to_value(params)?)?;

        // Send initialized notification
        self.send_notification("initialized", serde_json::json!({}))?;

        Ok(())
    }

    /// Ensure a file is opened on the LSP server via textDocument/didOpen.
    fn ensure_file_opened(&mut self, file_path: &str) -> anyhow::Result<()> {
        if self.opened_files.contains(file_path) {
            return Ok(());
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read {} for didOpen: {}", file_path, e);
                return Ok(());
            }
        };
        let language_id = detect_language_id(file_path);

        let uri = Uri::from_str(&format!("file://{}", file_path))
            .map_err(|e| anyhow::anyhow!("Invalid file path: {}: {}", file_path, e))?;

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_string(),
                version: 1,
                text: content,
            },
        };

        self.send_notification("textDocument/didOpen", serde_json::to_value(params)?)?;
        self.opened_files.insert(file_path.to_string());
        Ok(())
    }

    pub fn definition(
        &mut self,
        file_path: &str,
        line: u32,
        col: u32,
    ) -> anyhow::Result<Option<Location>> {
        // Ensure file is opened before requesting definition
        self.ensure_file_opened(file_path)?;

        let file_uri = Uri::from_str(&format!("file://{}", file_path))
            .map_err(|e| anyhow::anyhow!("Invalid file path: {}: {}", file_path, e))?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: file_uri },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let response =
            self.send_request("textDocument/definition", serde_json::to_value(params)?)?;

        match response {
            Some(value) => {
                if let Ok(resp) = serde_json::from_value::<GotoDefinitionResponse>(value) {
                    match resp {
                        GotoDefinitionResponse::Scalar(loc) => Ok(Some(loc)),
                        GotoDefinitionResponse::Array(locs) => Ok(locs.into_iter().next()),
                        GotoDefinitionResponse::Link(links) => {
                            Ok(links.into_iter().next().map(|link| Location {
                                uri: link.target_uri,
                                range: link.target_range,
                            }))
                        }
                    }
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub fn shutdown(&mut self) -> anyhow::Result<()> {
        let _ = self.send_request("shutdown", Value::Null);
        let _ = self.send_notification("exit", Value::Null);
        let _ = self.process.kill();
        let _ = self.process.wait();
        Ok(())
    }

    fn send_request(&mut self, method: &str, params: Value) -> anyhow::Result<Option<Value>> {
        // Check process health before sending
        {
            let last = *self.last_response_at.lock().unwrap();
            if last.elapsed() > PROCESS_HEALTH_TIMEOUT && self.next_id.load(Ordering::Relaxed) > 2 {
                // Process may be dead — try to kill and return error
                let _ = self.process.kill();
                return Err(anyhow::anyhow!(
                    "LSP process unresponsive for {}s, killed",
                    PROCESS_HEALTH_TIMEOUT.as_secs()
                ));
            }
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&request)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No stdin available"))?;
        stdin.write_all(message.as_bytes())?;
        stdin.flush()?;

        // Read response using channel with timeout
        self.read_response(id)
    }

    fn send_notification(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        let body = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))?;

        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No stdin available"))?;
        stdin.write_all(message.as_bytes())?;
        stdin.flush()?;

        Ok(())
    }

    fn read_response(&mut self, expected_id: i64) -> anyhow::Result<Option<Value>> {
        // Check buffer first for previously-received out-of-order responses
        if let Some(result) = self.pending_responses.remove(&expected_id) {
            return Ok(result);
        }

        let deadline = Instant::now() + RESPONSE_TIMEOUT;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow::anyhow!(
                    "Timeout: no response for request id {} within {}s",
                    expected_id,
                    RESPONSE_TIMEOUT.as_secs()
                ));
            }

            match self.response_rx.recv_timeout(remaining) {
                Ok(msg) => {
                    // Update last response timestamp
                    *self.last_response_at.lock().unwrap() = Instant::now();

                    if msg.id == Some(expected_id) {
                        return Ok(msg.result);
                    }
                    // Buffer non-matching responses (skip notifications which have no id)
                    if let Some(id) = msg.id {
                        self.pending_responses.insert(id, msg.result);
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    return Err(anyhow::anyhow!(
                        "Timeout: no response for request id {} within {}s",
                        expected_id,
                        RESPONSE_TIMEOUT.as_secs()
                    ));
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow::anyhow!(
                        "LSP reader thread disconnected (process may have exited)"
                    ));
                }
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Detect the LSP language ID from a file path extension.
fn detect_language_id(file_path: &str) -> &str {
    match file_path.rsplit('.').next() {
        Some("rs") => "rust",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => "javascript",
        Some("py") | Some("pyi") => "python",
        Some("c") | Some("h") => "c",
        Some("cpp") | Some("cxx") | Some("cc") | Some("hpp") | Some("hxx") => "cpp",
        Some("html") | Some("htm") => "html",
        _ => "plaintext",
    }
}
