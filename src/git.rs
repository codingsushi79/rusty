//! Git integration: branch, HEAD blob (for the diff gutter), status, stage,
//! commit, discard.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Per-file working-tree status letter (M/A/U/D/C), keyed by absolute path.
pub fn statuses(root: &Path) -> HashMap<PathBuf, char> {
    let mut map = HashMap::new();
    let Ok(repo) = git2::Repository::discover(root) else { return map };
    let Some(workdir) = repo.workdir().map(Path::to_path_buf) else { return map };
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let Ok(statuses) = repo.statuses(Some(&mut opts)) else { return map };
    for e in statuses.iter() {
        let Some(rel) = e.path() else { continue };
        let s = e.status();
        let letter = if s.is_conflicted() {
            'C'
        } else if s.is_wt_deleted() || s.is_index_deleted() {
            'D'
        } else if s.is_wt_new() {
            'U'
        } else if s.is_index_new() {
            'A'
        } else if s.is_wt_modified() || s.is_index_modified() || s.is_wt_renamed() || s.is_index_renamed() {
            'M'
        } else {
            continue;
        };
        let abs = workdir.join(rel);
        map.insert(abs.canonicalize().unwrap_or(abs), letter);
    }
    map
}

/// Local branch names (current branch first).
pub fn branches(root: &Path) -> Vec<String> {
    let Ok(repo) = git2::Repository::discover(root) else { return Vec::new() };
    let current = repo.head().ok().and_then(|h| h.shorthand().map(String::from));
    let mut names = Vec::new();
    if let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) {
        for b in branches.flatten() {
            if let Ok(Some(name)) = b.0.name() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    if let Some(cur) = current {
        names.retain(|n| n != &cur);
        names.insert(0, cur);
    }
    names
}

/// Check out an existing local branch.
pub fn checkout_branch(root: &Path, name: &str) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(root)?;
    let obj = repo.revparse_single(name)?;
    repo.checkout_tree(&obj, Some(git2::build::CheckoutBuilder::new().safe()))?;
    repo.set_head(&format!("refs/heads/{name}"))?;
    Ok(())
}

/// Create a new branch from HEAD and check it out.
pub fn create_branch(root: &Path, name: &str) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(root)?;
    let head = repo.head()?.peel_to_commit()?;
    repo.branch(name, &head, false)?;
    drop(head);
    checkout_branch(root, name)
}

pub fn delete_branch(root: &Path, name: &str) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(root)?;
    repo.find_branch(name, git2::BranchType::Local)?.delete()?;
    Ok(())
}

/// Rename the current branch.
pub fn rename_branch(root: &Path, new_name: &str) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(root)?;
    let head = repo.head()?;
    let current = head.shorthand().ok_or_else(|| anyhow::anyhow!("detached HEAD"))?.to_string();
    let mut b = repo.find_branch(&current, git2::BranchType::Local)?;
    b.rename(new_name, false)?;
    Ok(())
}

/// Discard working-tree changes to `file` (restore it from HEAD).
pub fn discard(file: &Path) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(file)?;
    let workdir = repo.workdir().ok_or_else(|| anyhow::anyhow!("bare repo"))?.to_path_buf();
    let rel = file.strip_prefix(&workdir)?;
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.path(rel);
    cb.force();
    repo.checkout_head(Some(&mut cb))?;
    Ok(())
}

pub fn branch(path: &Path) -> Option<String> {
    let repo = git2::Repository::discover(path).ok()?;
    let head = repo.head().ok()?;
    head.shorthand().map(String::from)
}

/// The committed (HEAD) text of `file`, or `None` if untracked / not in a repo.
pub fn head_text(file: &Path) -> Option<String> {
    let repo = git2::Repository::discover(file).ok()?;
    let workdir = repo.workdir()?;
    let rel = file.strip_prefix(workdir).ok()?;
    let tree = repo.head().ok()?.peel_to_tree().ok()?;
    let entry = tree.get_path(rel).ok()?;
    let obj = entry.to_object(&repo).ok()?;
    let blob = obj.as_blob()?;
    Some(String::from_utf8_lossy(blob.content()).to_string())
}

/// Initialize a new git repository at `root`.
pub fn init(root: &Path) -> anyhow::Result<()> {
    git2::Repository::init(root)?;
    Ok(())
}

/// Add (or update) a named remote.
pub fn add_remote(root: &Path, name: &str, url: &str) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(root)?;
    if repo.find_remote(name).is_ok() {
        repo.remote_set_url(name, url)?;
    } else {
        repo.remote(name, url)?;
    }
    Ok(())
}

/// Stage every change in the working tree (`git add -A`).
pub fn stage_all(root: &Path) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(root)?;
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    Ok(())
}

/// Stage a single file (`git add <file>`).
pub fn stage(file: &Path) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(file)?;
    let workdir = repo.workdir().ok_or_else(|| anyhow::anyhow!("bare repo"))?.to_path_buf();
    let rel = file.strip_prefix(&workdir)?;
    let mut index = repo.index()?;
    index.add_path(rel)?;
    index.write()?;
    Ok(())
}

/// Commit the current index with `message`.
pub fn commit(path: &Path, message: &str) -> anyhow::Result<git2::Oid> {
    let repo = git2::Repository::discover(path)?;
    let mut index = repo.index()?;
    let tree = repo.find_tree(index.write_tree()?)?;
    let sig = repo.signature()?;
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    Ok(repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?)
}
