use std::{
    env,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnectionStatus {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub detail: String,
}

pub fn detect() -> Vec<AgentConnectionStatus> {
    let paths = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    let extensions = command_extensions();
    [
        ("codex", "Codex", "codex"),
        ("claude-code", "Claude Code", "claude"),
        ("kimi-code", "Kimi Code", "kimi"),
    ]
    .into_iter()
    .map(|(id, name, command)| {
        let available = command_available_in(command, &paths, &extensions);
        AgentConnectionStatus {
            id: id.into(),
            name: name.into(),
            available,
            detail: if available {
                format!("已检测到 {command} 本地命令")
            } else {
                format!("未在 PATH 中检测到 {command}")
            },
        }
    })
    .collect()
}

fn command_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(str::trim)
            .filter(|extension| !extension.is_empty())
            .map(|extension| extension.to_ascii_lowercase())
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn command_available_in(command: &str, paths: &[PathBuf], extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return paths.iter().any(|path| is_file(&path.join(command)));
    }

    paths.iter().any(|path| {
        is_file(&path.join(command))
            || extensions.iter().any(|extension| {
                is_file(&path.join(format!("{command}{extension}")))
                    || is_file(&path.join(format!("{command}{}", extension.to_ascii_uppercase())))
            })
    })
}

fn is_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_commands_in_path_with_windows_extensions() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("codex.cmd"), "fixture").unwrap();

        assert!(command_available_in(
            "codex",
            &[directory.path().to_path_buf()],
            &[".cmd".into()]
        ));
        assert!(!command_available_in(
            "claude",
            &[directory.path().to_path_buf()],
            &[".cmd".into()]
        ));
    }
}
