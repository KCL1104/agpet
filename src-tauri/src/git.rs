//! Minimal git-worktree helpers (shells out to `git`; no extra crate).
//!
//! Used by the "parallel / vertical task" feature: each worker pet runs in its
//! own linked worktree so concurrent agents don't clobber the same tree.
//! Worktrees live under `<repo>/.agpet-worktrees/<branch-slug>` and are ignored
//! via `<repo>/.git/info/exclude` (so we never touch the tracked `.gitignore`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
}

/// Run `git` in `dir`, returning stdout on success or a readable error.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Whether `dir` is inside a git work tree.
pub fn is_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Filesystem slug for a branch name (keeps it path-safe).
fn path_slug(branch: &str) -> String {
    branch.replace(['/', '\\', ':'], "-")
}

/// Make sure `.agpet-worktrees/` is locally ignored (via `.git/info/exclude`,
/// not the tracked `.gitignore`).
fn ensure_excluded(repo: &Path) -> Result<(), String> {
    let git_dir = git(repo, &["rev-parse", "--git-common-dir"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| ".git".to_string());
    let mut p = PathBuf::from(&git_dir);
    if p.is_relative() {
        p = repo.join(p);
    }
    let exclude = p.join("info").join("exclude");
    let entry = ".agpet-worktrees/";
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !existing.lines().any(|l| l.trim() == entry) {
        if let Some(parent) = exclude.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut body = existing;
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(entry);
        body.push('\n');
        std::fs::write(&exclude, body).map_err(|e| format!("write exclude: {e}"))?;
    }
    Ok(())
}

/// Create a new worktree on a new branch off the current HEAD. If `branch`
/// already exists, a numeric suffix is appended. Returns the worktree path.
pub fn worktree_create(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    if !is_repo(repo) {
        return Err(format!("{} is not a git repository", repo.display()));
    }
    ensure_excluded(repo)?;
    let base = repo.join(".agpet-worktrees");
    std::fs::create_dir_all(&base).map_err(|e| format!("create worktrees dir: {e}"))?;

    // Find a free branch + path (suffix on collision).
    let mut name = branch.to_string();
    let mut path = base.join(path_slug(&name));
    let mut n = 2;
    while branch_exists(repo, &name) || path.exists() {
        name = format!("{branch}-{n}");
        path = base.join(path_slug(&name));
        n += 1;
        if n > 50 {
            return Err("could not find a free branch name".into());
        }
    }

    git(
        repo,
        &["worktree", "add", "-b", &name, &path.to_string_lossy(), "HEAD"],
    )?;
    Ok(path)
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok()
}

/// List the repo's worktrees (path + branch).
pub fn worktree_list(repo: &Path) -> Result<Vec<WorktreeInfo>, String> {
    let out = git(repo, &["worktree", "list", "--porcelain"])?;
    let mut list = Vec::new();
    let mut cur_path: Option<String> = None;
    let mut cur_branch = String::new();
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            cur_path = Some(p.trim().to_string());
            cur_branch = String::new();
        } else if let Some(b) = line.strip_prefix("branch ") {
            cur_branch = b.trim().trim_start_matches("refs/heads/").to_string();
        } else if line.trim().is_empty() {
            if let Some(p) = cur_path.take() {
                list.push(WorktreeInfo { path: p, branch: cur_branch.clone() });
            }
        }
    }
    if let Some(p) = cur_path.take() {
        list.push(WorktreeInfo { path: p, branch: cur_branch });
    }
    // Only show the worktrees we manage.
    list.retain(|w| w.path.replace('\\', "/").contains("/.agpet-worktrees/"));
    Ok(list)
}

/// Remove a worktree (refuses if it has uncommitted changes — no `--force`).
pub fn worktree_remove(repo: &Path, path: &str) -> Result<(), String> {
    git(repo, &["worktree", "remove", path])?;
    Ok(())
}

/// Stage everything and commit in the worktree at `wt_path` (commits to that
/// worktree's branch). Errors if there's nothing to commit.
pub fn worktree_commit(wt_path: &Path, message: &str) -> Result<String, String> {
    git(wt_path, &["add", "-A"])?;
    let out = Command::new("git")
        .arg("-C")
        .arg(wt_path)
        .args(["commit", "-m", message])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        let summary = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("committed")
            .trim()
            .to_string();
        Ok(format!("Committed — {summary}"))
    } else {
        // `git commit` prints "nothing to commit…" to stdout, errors to stderr.
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = if stderr.trim().is_empty() { stdout } else { stderr };
        Err(msg.lines().find(|l| !l.trim().is_empty()).unwrap_or("commit failed").trim().to_string())
    }
}

/// Merge `branch` into the base repo's current branch (only brings in commits
/// the worktree actually made). On conflict the merge is aborted so the repo
/// stays clean and the user is told to merge manually.
pub fn worktree_merge(repo: &Path, branch: &str) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge", "--no-edit", branch])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        let summary = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("merged")
            .trim()
            .to_string();
        Ok(format!("Merged {branch} — {summary}"))
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let _ = git(repo, &["merge", "--abort"]); // clean up if a conflicted merge started
        Err(if err.is_empty() {
            format!("merge of {branch} failed — resolve manually")
        } else {
            err
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_slug_replaces_separators() {
        assert_eq!(path_slug("agpet/delegate/claude-1"), "agpet-delegate-claude-1");
        assert_eq!(path_slug("feature\\x"), "feature-x");
        assert_eq!(path_slug("a:b"), "a-b");
        assert_eq!(path_slug("plain"), "plain");
    }
}
