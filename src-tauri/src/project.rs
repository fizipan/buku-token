use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::i18n;
use crate::settings::Lang;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub path: String,
    pub name: String,
    pub display_path: String,
}

pub fn resolve(
    cwd: &str,
    home: &Path,
    git_root: impl Fn(&Path) -> Option<PathBuf>,
) -> ProjectRef {
    if cwd.is_empty() {
        return ProjectRef {
            path: String::new(),
            name: i18n::no_project(Lang::default()).into(),
            display_path: "–".into(),
        };
    }
    let cwd_path = Path::new(cwd);
    let root = git_root(cwd_path).unwrap_or_else(|| cwd_path.to_path_buf());
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let display_path = display_path(&root, home);
    ProjectRef { path: root.to_string_lossy().into_owned(), name, display_path }
}

fn display_path(root: &Path, home: &Path) -> String {
    if let Ok(rel) = root.strip_prefix(home.join("Documents"))
        && !rel.as_os_str().is_empty()
    {
        return rel.to_string_lossy().into_owned();
    }
    match root.strip_prefix(home) {
        Ok(rel) if rel.as_os_str().is_empty() => "~".into(),
        Ok(rel) => format!("~/{}", rel.to_string_lossy()),
        Err(_) => root.to_string_lossy().into_owned(),
    }
}

/// Walks up from `start` looking for a `.git` file or directory.
/// `stop_at` itself is never checked (a `.git` in $HOME must not swallow every project).
pub fn find_git_root(start: &Path, stop_at: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir == stop_at {
            return None;
        }
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

pub struct ProjectResolver {
    home: PathBuf,
    cache: HashMap<String, ProjectRef>,
}

impl ProjectResolver {
    pub fn new(home: PathBuf) -> Self {
        Self { home, cache: HashMap::new() }
    }

    /// The project of `cwd`. The cache keeps no language: "no project" is named in `lang` on the way out.
    pub fn resolve(&mut self, cwd: &str, lang: Lang) -> ProjectRef {
        let mut r = match self.cache.get(cwd) {
            Some(hit) => hit.clone(),
            None => {
                let home = self.home.clone();
                let r = resolve(cwd, &home, |p| find_git_root(p, &home));
                self.cache.insert(cwd.to_string(), r.clone());
                r
            }
        };
        if r.path.is_empty() {
            r.name = i18n::no_project(lang).into();
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/dev")
    }

    #[test]
    fn subfolder_resolves_to_git_root() {
        let root = PathBuf::from("/Users/dev/Documents/Acme/cms-klien-a");
        let r = resolve(
            "/Users/dev/Documents/Acme/cms-klien-a/src/components",
            &home(),
            |_| Some(root.clone()),
        );
        assert_eq!(r.path, "/Users/dev/Documents/Acme/cms-klien-a");
        assert_eq!(r.name, "cms-klien-a");
    }

    #[test]
    fn no_git_root_falls_back_to_cwd() {
        let r = resolve("/Users/dev/dev/x", &home(), |_| None);
        assert_eq!(r.path, "/Users/dev/dev/x");
        assert_eq!(r.name, "x");
    }

    #[test]
    fn display_path_relative_to_documents() {
        let r = resolve("/Users/dev/Documents/Acme/cms-klien-a", &home(), |_| None);
        assert_eq!(r.display_path, "Acme/cms-klien-a");
    }

    #[test]
    fn outside_documents_uses_tilde() {
        let r = resolve("/Users/dev/dev/x", &home(), |_| None);
        assert_eq!(r.display_path, "~/dev/x");
    }

    #[test]
    fn outside_home_uses_absolute_path() {
        let r = resolve("/opt/work/y", &home(), |_| None);
        assert_eq!(r.display_path, "/opt/work/y");
    }

    #[test]
    fn empty_cwd_is_tanpa_project() {
        let r = resolve("", &home(), |_| panic!("must not look up git"));
        assert_eq!(r.path, "");
        assert_eq!(r.name, "Tanpa Project");
        assert_eq!(r.display_path, "–");
    }

    #[test]
    fn no_project_is_named_in_the_language_asked_without_touching_the_cache() {
        let mut res = ProjectResolver::new(home());
        assert_eq!(res.resolve("", Lang::Id).name, "Tanpa Project");
        assert_eq!(res.resolve("", Lang::En).name, "No Project");
        assert_eq!(res.resolve("", Lang::Id).name, "Tanpa Project");
        assert_eq!(res.resolve("", Lang::En).path, "");
    }

    #[test]
    fn find_git_root_walks_up_and_stops_at_home() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_path_buf();
        let repo = home.join("proj");
        let deep = repo.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir(repo.join(".git")).unwrap();
        assert_eq!(find_git_root(&deep, &home), Some(repo.clone()));

        // `.git` as a file (worktree/submodule) counts too.
        let wt = home.join("wt/sub");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(home.join("wt/.git"), "gitdir: x").unwrap();
        assert_eq!(find_git_root(&wt, &home), Some(home.join("wt")));

        // No repo below home; a .git at home itself is ignored.
        let other = home.join("plain/dir");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir(home.join(".git")).unwrap();
        assert_eq!(find_git_root(&other, &home), None);
    }

    #[test]
    fn resolver_caches_per_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("proj");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let sub = repo.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        let mut res = ProjectResolver::new(tmp.path().to_path_buf());
        let a = res.resolve(sub.to_str().unwrap(), Lang::Id);
        assert_eq!(a.name, "proj");
        std::fs::remove_dir_all(repo.join(".git")).unwrap();
        let b = res.resolve(sub.to_str().unwrap(), Lang::Id);
        assert_eq!(a, b);
    }
}
