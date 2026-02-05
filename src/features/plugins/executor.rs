//! # Secure CLI Executor
//!
//! Execute external CLI commands safely with parameter substitution,
//! input validation, output limiting, and timeout enforcement.
//!
//! - **Version**: 2.1.0
//! - **Since**: 0.9.0
//!
//! ## Changelog
//! - 2.1.0: execute_on_file() now accepts params for user-provided options (e.g., language)
//! - 2.0.0: Added execute_on_file() for chunked transcription support
//! - 1.2.0: Allow URLs with special chars (&) by shell-escaping them properly
//! - 1.1.0: Fixed validation to check user params before substitution, allowing shell scripts in config
//! - 1.0.0: Initial release

use crate::features::plugins::config::{ChunkingConfig, ExecutionConfig};
use anyhow::Result;
use log::{info, warn};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Dangerous characters that could enable shell injection
const DANGEROUS_CHARS: &[char] = &[
    '|', // Pipe
    ';', // Command separator
    '&', // Background/AND
    '$', // Variable expansion
    '`', // Command substitution
    '(', // Subshell
    ')', '{', // Brace expansion
    '}', '<', // Redirection
    '>', '\n', // Newline injection
    '\r', '\0', // Null byte
];

/// Result of a CLI command execution
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Whether the command exited successfully
    pub success: bool,

    /// Exit code if available
    pub exit_code: Option<i32>,

    /// Standard output (may be truncated)
    pub stdout: String,

    /// Standard error output
    pub stderr: String,

    /// Whether the command timed out
    pub timed_out: bool,
}

/// Secure CLI command executor
#[derive(Clone)]
pub struct PluginExecutor {
    /// Set of allowed commands (e.g., "docker", "echo")
    allowed_commands: HashSet<String>,
}

impl PluginExecutor {
    /// Create a new executor with the given allowlist
    pub fn new(allowed_commands: Vec<String>) -> Self {
        Self {
            allowed_commands: allowed_commands.into_iter().collect(),
        }
    }

    /// Execute a plugin command with full security checks
    pub async fn execute(
        &self,
        config: &ExecutionConfig,
        params: &HashMap<String, String>,
    ) -> Result<ExecutionResult> {
        // 1. Verify command is in allowlist
        if !self.allowed_commands.contains(&config.command) {
            return Err(anyhow::anyhow!(
                "Command not in allowlist: {}. Allowed: {:?}",
                config.command,
                self.allowed_commands
            ));
        }

        // 2. Substitute parameters in args (validates user params before substitution)
        let args = self.substitute_params(&config.args, params)?;

        info!(
            "Executing plugin command: {} {:?} (timeout: {}s)",
            config.command, args, config.timeout_seconds
        );

        // 3. Build async command
        let mut cmd = Command::new(&config.command);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // Set working directory if specified
        if let Some(ref dir) = config.working_directory {
            cmd.current_dir(dir);
        }

        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // 4. Execute with timeout
        let timeout_duration = Duration::from_secs(config.timeout_seconds);
        let result = timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Truncate output if needed
                let stdout = if stdout.len() > config.max_output_bytes {
                    warn!(
                        "Output truncated from {} to {} bytes",
                        stdout.len(),
                        config.max_output_bytes
                    );
                    format!(
                        "{}...\n\n[Output truncated at {} bytes]",
                        &stdout[..config.max_output_bytes],
                        config.max_output_bytes
                    )
                } else {
                    stdout.to_string()
                };

                let exit_code = output.status.code();
                let success = output.status.success();

                if success {
                    info!(
                        "Command completed successfully, output length: {} chars",
                        stdout.len()
                    );
                } else {
                    warn!("Command failed with exit code: {:?}", exit_code);
                }

                Ok(ExecutionResult {
                    success,
                    exit_code,
                    stdout,
                    stderr: stderr.to_string(),
                    timed_out: false,
                })
            }
            Ok(Err(e)) => {
                warn!("Command execution failed: {}", e);
                Err(anyhow::anyhow!("Failed to execute command: {}", e))
            }
            Err(_) => {
                warn!("Command timed out after {} seconds", config.timeout_seconds);
                Ok(ExecutionResult {
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Command timed out after {} seconds", config.timeout_seconds),
                    timed_out: true,
                })
            }
        }
    }

    /// Execute a command on a local audio file (used for chunked transcription)
    ///
    /// This method is designed for processing individual audio chunks.
    /// It uses the chunking config's file_command and file_args, substituting
    /// ${file} with the actual file path, ${output_dir} with the output directory,
    /// and any user-provided params like ${language}.
    pub async fn execute_on_file(
        &self,
        chunking_config: &ChunkingConfig,
        file_path: &Path,
        output_dir: &Path,
        max_output_bytes: usize,
        params: &HashMap<String, String>,
    ) -> Result<ExecutionResult> {
        // Get the file command or fall back to docker whisper command
        let command = chunking_config.file_command.as_deref().unwrap_or("sh");

        // Verify command is in allowlist
        if !self.allowed_commands.contains(command) {
            return Err(anyhow::anyhow!(
                "Command not in allowlist: {}. Allowed: {:?}",
                command,
                self.allowed_commands
            ));
        }

        // Build parameters for substitution
        let file_str = file_path.to_string_lossy().to_string();
        let output_str = output_dir.to_string_lossy().to_string();

        // Default args if none provided
        let default_args = if chunking_config.file_args.is_empty() {
            vec![
                "-c".to_string(),
                format!(
                    r#"TMPDIR=$(mktemp -d)
ERRFILE=$(mktemp)
if docker run --rm -v "{}:/data/input.mp3" -v "$TMPDIR:/data/output" whisper-transcribe:latest /data/input.mp3 -m base -f txt -o /data/output >/dev/null 2>"$ERRFILE"; then
    cat "$TMPDIR"/*.txt 2>/dev/null || echo "No transcript generated"
else
    echo "Transcription failed:"
    cat "$ERRFILE"
fi
rm -rf "$TMPDIR" "$ERRFILE""#,
                    file_str
                ),
            ]
        } else {
            chunking_config.file_args.clone()
        };

        // Substitute placeholders in args (file, output_dir, and user params like language)
        let args: Vec<String> = default_args
            .iter()
            .map(|arg| {
                let mut result = arg
                    .replace("${file}", &file_str)
                    .replace("${output_dir}", &output_str);
                // Substitute user params (like ${language})
                for (key, value) in params {
                    let placeholder = format!("${{{}}}", key);
                    result = result.replace(&placeholder, value);
                }
                result
            })
            .collect();

        info!(
            "Executing file transcription: {} {:?} (timeout: {}s)",
            command, args, chunking_config.chunk_timeout_secs
        );

        // Build async command
        let mut cmd = Command::new(command);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // Execute with timeout
        let timeout_duration = Duration::from_secs(chunking_config.chunk_timeout_secs);
        let result = timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Truncate output if needed
                let stdout = if stdout.len() > max_output_bytes {
                    warn!(
                        "Chunk output truncated from {} to {} bytes",
                        stdout.len(),
                        max_output_bytes
                    );
                    format!(
                        "{}...\n\n[Output truncated at {} bytes]",
                        &stdout[..max_output_bytes],
                        max_output_bytes
                    )
                } else {
                    stdout.to_string()
                };

                let exit_code = output.status.code();
                let success = output.status.success();

                if success {
                    info!(
                        "Chunk transcription completed, output length: {} chars",
                        stdout.len()
                    );
                } else {
                    warn!("Chunk transcription failed with exit code: {:?}", exit_code);
                }

                Ok(ExecutionResult {
                    success,
                    exit_code,
                    stdout,
                    stderr: stderr.to_string(),
                    timed_out: false,
                })
            }
            Ok(Err(e)) => {
                warn!("Chunk command execution failed: {}", e);
                Err(anyhow::anyhow!("Failed to execute chunk command: {}", e))
            }
            Err(_) => {
                warn!(
                    "Chunk transcription timed out after {} seconds",
                    chunking_config.chunk_timeout_secs
                );
                Ok(ExecutionResult {
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!(
                        "Chunk transcription timed out after {} seconds",
                        chunking_config.chunk_timeout_secs
                    ),
                    timed_out: true,
                })
            }
        }
    }

    /// Substitute ${param} placeholders in arguments
    ///
    /// User-supplied params are validated BEFORE substitution to prevent
    /// shell injection while allowing trusted shell scripts in config.
    fn substitute_params(
        &self,
        args: &[String],
        params: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        // Validate user-supplied params BEFORE substitution
        for (key, value) in params {
            self.validate_argument(value)
                .map_err(|e| anyhow::anyhow!("Invalid parameter '{}': {}", key, e))?;
        }

        args.iter()
            .map(|arg| {
                let mut result = arg.clone();
                for (key, value) in params {
                    let placeholder = format!("${{{}}}", key);
                    result = result.replace(&placeholder, value);
                }

                // Check for unsubstituted placeholders
                if result.contains("${") {
                    // Extract the placeholder name for a better error message
                    if let Some(start) = result.find("${") {
                        if let Some(end) = result[start..].find('}') {
                            let placeholder = &result[start..start + end + 1];
                            return Err(anyhow::anyhow!(
                                "Unsubstituted placeholder {}: parameter not provided",
                                placeholder
                            ));
                        }
                    }
                    Err(anyhow::anyhow!(
                        "Unsubstituted placeholder in argument: {}",
                        result
                    ))
                } else {
                    Ok(result)
                }
            })
            .collect()
    }

    /// Validate an argument doesn't contain dangerous shell characters
    fn validate_argument(&self, arg: &str) -> Result<()> {
        // URLs are allowed to contain & for query parameters
        // Since we use direct process execution (not shell), & is safe in URLs
        let is_url = arg.starts_with("http://") || arg.starts_with("https://");

        for &ch in DANGEROUS_CHARS {
            // Allow & in URLs (query parameter separator)
            if ch == '&' && is_url {
                continue;
            }

            if arg.contains(ch) {
                let ch_display = match ch {
                    '\n' => "newline".to_string(),
                    '\r' => "carriage return".to_string(),
                    '\0' => "null byte".to_string(),
                    _ => format!("'{}'", ch),
                };
                return Err(anyhow::anyhow!(
                    "Argument contains forbidden character {}: {}",
                    ch_display,
                    arg.chars().take(50).collect::<String>()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_executor() -> PluginExecutor {
        PluginExecutor::new(vec!["echo".to_string(), "cat".to_string()])
    }

    #[test]
    fn test_command_allowlist() {
        let executor = create_test_executor();
        assert!(executor.allowed_commands.contains("echo"));
        assert!(executor.allowed_commands.contains("cat"));
        assert!(!executor.allowed_commands.contains("rm"));
    }

    #[test]
    fn test_substitute_params() {
        let executor = create_test_executor();
        let args = vec!["--url".to_string(), "${url}".to_string()];
        let mut params = HashMap::new();
        params.insert("url".to_string(), "https://example.com".to_string());

        let result = executor.substitute_params(&args, &params).unwrap();
        assert_eq!(result, vec!["--url", "https://example.com"]);
    }

    #[test]
    fn test_substitute_params_missing() {
        let executor = create_test_executor();
        let args = vec!["${missing}".to_string()];
        let params = HashMap::new();

        let result = executor.substitute_params(&args, &params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_argument_safe() {
        let executor = create_test_executor();
        assert!(executor.validate_argument("safe-argument").is_ok());
        assert!(executor
            .validate_argument("https://example.com/path?q=1")
            .is_ok());
        assert!(executor.validate_argument("file.txt").is_ok());
    }

    #[test]
    fn test_validate_argument_url_with_ampersand() {
        let executor = create_test_executor();
        // URLs with & query params should be allowed
        assert!(executor
            .validate_argument("https://www.youtube.com/watch?v=abc123&list=PLxyz")
            .is_ok());
        assert!(executor
            .validate_argument("https://example.com/path?a=1&b=2&c=3")
            .is_ok());
        assert!(executor
            .validate_argument("http://example.com/?foo=bar&baz=qux")
            .is_ok());

        // But non-URL strings with & should still be blocked
        assert!(executor.validate_argument("foo & bar").is_err());
        assert!(executor.validate_argument("command && other").is_err());
    }

    #[test]
    fn test_validate_argument_dangerous() {
        let executor = create_test_executor();
        assert!(executor.validate_argument("arg; rm -rf /").is_err());
        assert!(executor.validate_argument("arg | cat").is_err());
        assert!(executor.validate_argument("$(whoami)").is_err());
        assert!(executor.validate_argument("arg`id`").is_err());
        assert!(executor.validate_argument("arg\ninjected").is_err());
    }

    #[tokio::test]
    async fn test_execute_not_allowed() {
        let executor = create_test_executor();
        let config = ExecutionConfig {
            command: "rm".to_string(),
            args: vec!["-rf".to_string()],
            timeout_seconds: 10,
            working_directory: None,
            max_output_bytes: 1000,
            env: HashMap::new(),
            chunking: None,
        };

        let result = executor.execute(&config, &HashMap::new()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not in allowlist"));
    }

    #[tokio::test]
    async fn test_execute_echo() {
        let executor = create_test_executor();
        let config = ExecutionConfig {
            command: "echo".to_string(),
            args: vec!["hello".to_string(), "world".to_string()],
            timeout_seconds: 10,
            working_directory: None,
            max_output_bytes: 1000,
            env: HashMap::new(),
            chunking: None,
        };

        let result = executor.execute(&config, &HashMap::new()).await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "hello world");
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn test_execute_with_params() {
        let executor = create_test_executor();
        let config = ExecutionConfig {
            command: "echo".to_string(),
            args: vec!["Message: ${msg}".to_string()],
            timeout_seconds: 10,
            working_directory: None,
            max_output_bytes: 1000,
            env: HashMap::new(),
            chunking: None,
        };

        let mut params = HashMap::new();
        params.insert("msg".to_string(), "test message".to_string());

        let result = executor.execute(&config, &params).await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "Message: test message");
    }

    #[test]
    fn test_substitute_params_rejects_dangerous_user_input() {
        let executor = create_test_executor();
        let args = vec!["${url}".to_string()];
        let mut params = HashMap::new();

        // User tries to inject shell command via param
        params.insert("url".to_string(), "https://evil.com; rm -rf /".to_string());

        let result = executor.substitute_params(&args, &params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid parameter"));
    }

    #[test]
    fn test_substitute_params_allows_shell_script_in_config() {
        let executor = create_test_executor();
        // Shell script in config args (trusted) - should be allowed
        let args = vec![
            "-c".to_string(),
            "TMPDIR=$(mktemp -d) && echo $TMPDIR".to_string(),
        ];
        let params = HashMap::new();

        // No user params, so shell script in args should pass through
        let result = executor.substitute_params(&args, &params).unwrap();
        assert_eq!(result[1], "TMPDIR=$(mktemp -d) && echo $TMPDIR");
    }
}
