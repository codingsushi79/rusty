//! Git integration: branch, HEAD blob (for the diff gutter), stage, commit.

use std::path::Path;

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
