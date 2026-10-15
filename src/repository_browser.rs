use crate::git;
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
};

struct HistoryTask {
    receiver: Receiver<Result<Vec<git::Commit>, String>>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for HistoryTask {
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) struct HistoryTab {
    pub path: String,
    pub commits: Vec<git::Commit>,
    pub selected_hash: String,
    pub search: String,
    pub error: Option<String>,
    task: Option<HistoryTask>,
}
impl HistoryTab {
    pub fn loading(&self) -> bool {
        self.task.is_some()
    }
    fn reload(&mut self, root: &Path) {
        let root = root.to_path_buf();
        let path = self.path.clone();
        self.error = None;
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        self.task = Some(HistoryTask {
            receiver,
            cancelled,
        });
        thread::spawn(move || {
            let _ = sender.send(
                git::load_file_history(&root, &path, &worker_cancelled)
                    .map_err(|error| error.to_string()),
            );
        });
    }
    fn poll(&mut self) {
        let Some(task) = &self.task else {
            return;
        };
        let result = match task.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("File history task disconnected".into()),
        };
        self.task = None;
        match result {
            Ok(commits) => {
                if !commits
                    .iter()
                    .any(|commit| commit.hash == self.selected_hash)
                {
                    self.selected_hash = commits
                        .first()
                        .map(|commit| commit.hash.clone())
                        .unwrap_or_default();
                }
                self.commits = commits;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

pub(crate) fn is_hidden_path(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}

enum FileFilter {
    All,
    Fuzzy { text: String, path: bool },
    Glob(Option<regex::Regex>),
}
impl FileFilter {
    fn new(query: &str) -> Self {
        let text = query.trim().replace('\\', "/").to_lowercase();
        if text.is_empty() {
            return Self::All;
        }
        if !text.contains(['*', '?']) {
            return Self::Fuzzy {
                path: text.contains('/'),
                text,
            };
        }
        let mut expression = String::from("(?:^|/)");
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '*' if chars.peek() == Some(&'*') => {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        expression.push_str("(?:.*/)?");
                    } else {
                        expression.push_str(".*");
                    }
                }
                '*' => expression.push_str("[^/]*"),
                '?' => expression.push_str("[^/]"),
                ch => expression.push_str(&regex::escape(&ch.to_string())),
            }
        }
        expression.push('$');
        Self::Glob(regex::Regex::new(&expression).ok())
    }
    fn matches(&self, path: &str) -> bool {
        fn fuzzy(candidate: &str, needle: &str) -> bool {
            let mut remaining = candidate.chars();
            needle
                .chars()
                .all(|ch| remaining.by_ref().any(|next| ch == next))
        }
        match self {
            Self::All => true,
            Self::Glob(regex) => regex
                .as_ref()
                .is_some_and(|regex| regex.is_match(&path.to_lowercase())),
            Self::Fuzzy {
                text,
                path: full_path,
            } => {
                let path = path.to_lowercase();
                if *full_path {
                    fuzzy(&path, text)
                } else {
                    path.split('/').any(|part| fuzzy(part, text))
                }
            }
        }
    }
}

pub(crate) fn list_files(root: &Path) -> Result<Vec<String>, String> {
    let result = git::git_command()
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned());
    }
    let mut paths = result
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[derive(Default)]
pub(crate) struct Browser {
    pub root: Option<PathBuf>,
    pub files: Vec<git::WorktreeFile>,
    pub selected: Option<String>,
    pub error: Option<String>,
    list_task: Option<Receiver<Result<Vec<String>, String>>>,
    pub collapsed: std::collections::HashSet<String>,
    known_directories: std::collections::HashSet<String>,
    pub filter_collapsed: std::collections::HashSet<String>,
    pub filter: String,
    pub show_hidden: bool,
    pub visible_files: Vec<git::WorktreeFile>,
    pub tabs: Vec<HistoryTab>,
    pub active_tab: Option<usize>,
    pub dragging_tab: Option<usize>,
}

impl Browser {
    pub fn reload(&mut self, root: PathBuf) {
        let same = self.root.as_ref() == Some(&root);
        let mut tabs = if same {
            std::mem::take(&mut self.tabs)
        } else {
            Vec::new()
        };
        let active_tab = if same { self.active_tab } else { None };
        let show_hidden = self.show_hidden;
        let filter = self.filter.clone();
        let known_directories = if same {
            std::mem::take(&mut self.known_directories)
        } else {
            Default::default()
        };
        let filter_collapsed = if same {
            std::mem::take(&mut self.filter_collapsed)
        } else {
            Default::default()
        };
        if let Some(tab) = active_tab.and_then(|index| tabs.get_mut(index)) {
            tab.reload(&root);
        }
        let selected = if same { self.selected.take() } else { None };
        let collapsed = if same {
            std::mem::take(&mut self.collapsed)
        } else {
            Default::default()
        };
        *self = Self {
            root: Some(root.clone()),
            selected,
            collapsed,
            tabs,
            active_tab,
            show_hidden,
            filter,
            known_directories,
            filter_collapsed,
            ..Self::default()
        };
        let (send, receive) = mpsc::channel();
        self.list_task = Some(receive);
        thread::spawn(move || {
            let _ = send.send(list_files(&root));
        });
    }
    pub fn busy(&self) -> bool {
        self.list_task.is_some() || self.tabs.iter().any(HistoryTab::loading)
    }
    pub fn listing(&self) -> bool {
        self.list_task.is_some()
    }
    pub fn poll(&mut self) {
        for tab in &mut self.tabs {
            tab.poll();
        }
        use mpsc::TryRecvError;
        if let Some(task) = &self.list_task {
            match task.try_recv() {
                Ok(result) => {
                    self.list_task = None;
                    match result {
                        Ok(files) => {
                            self.files = files
                                .into_iter()
                                .map(|path| git::WorktreeFile {
                                    display_path: path.clone(),
                                    path,
                                    index_status: ' ',
                                    worktree_status: ' ',
                                })
                                .collect();
                            self.initialize_directory_state();
                            self.rebuild_visible_files();
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.list_task = None;
                    self.error = Some("File listing task disconnected".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    fn rebuild_visible_files(&mut self) {
        let matcher = FileFilter::new(&self.filter);
        self.visible_files = self
            .files
            .iter()
            .filter(|file| self.show_hidden || !is_hidden_path(&file.path))
            .filter(|file| matcher.matches(&file.path))
            .cloned()
            .collect();
    }
    fn initialize_directory_state(&mut self) {
        for file in &self.files {
            for (index, _) in file.path.match_indices('/') {
                let directory = &file.path[..index];
                if self.known_directories.insert(directory.to_owned()) {
                    self.collapsed.insert(directory.to_owned());
                }
            }
        }
    }
    pub fn set_filter(&mut self, filter: String) {
        if self.filter != filter {
            self.filter = filter;
            self.filter_collapsed.clear();
            self.rebuild_visible_files();
        }
    }
    pub fn set_show_hidden(&mut self, value: bool) {
        self.show_hidden = value;
        self.rebuild_visible_files();
    }
    pub fn open_history(&mut self, path: String) {
        let Some(_) = self.root.as_ref() else {
            return;
        };
        let index = if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
            index
        } else {
            let tab = HistoryTab {
                path: path.clone(),
                commits: Vec::new(),
                selected_hash: String::new(),
                search: String::new(),
                error: None,
                task: None,
            };
            self.tabs.push(tab);
            self.tabs.len() - 1
        };
        self.activate(index);
    }
    pub fn activate(&mut self, index: usize) {
        let changed = self.active_tab != Some(index);
        if let Some(tab) = self.tabs.get_mut(index) {
            self.selected = Some(tab.path.clone());
            self.active_tab = Some(index);
            if changed || !tab.loading() {
                if let Some(root) = &self.root {
                    tab.reload(root);
                }
            }
        }
    }
    pub fn close(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let previous_path = self.selected.clone();
        self.tabs.remove(index);
        let next = self.active_tab.map(|active| {
            if active > index {
                active - 1
            } else if active == index {
                index.min(self.tabs.len().saturating_sub(1))
            } else {
                active
            }
        });
        self.active_tab = None;
        self.selected = None;
        if !self.tabs.is_empty() {
            let next = next.unwrap_or(0);
            if previous_path.as_deref() == Some(self.tabs[next].path.as_str()) {
                self.active_tab = Some(next);
                self.selected = previous_path;
            } else {
                self.activate(next);
            }
        }
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let selected = self.selected.clone();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active_tab = selected
            .as_ref()
            .and_then(|path| self.tabs.iter().position(|tab| &tab.path == path));
        self.dragging_tab = Some(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("git-agent-browser-{name}-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn file_history_tabs_follow_renames_keep_state_and_cancel_on_close() {
        let root = fixture("history-tabs");
        let git = |args: &[&str]| {
            let out = git::git_command()
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{:?}", out);
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "History Test"]);
        git(&["config", "user.email", "history@example.invalid"]);
        fs::write(root.join("a.txt"), "original\n").unwrap();
        fs::write(root.join("b.txt"), "other file\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        fs::write(root.join("a.txt"), "edited before rename\n").unwrap();
        git(&["commit", "-am", "edit a"]);
        git(&["mv", "a.txt", "renamed.txt"]);
        git(&["commit", "-m", "rename a"]);
        fn wait(browser: &mut Browser) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while browser.busy() && std::time::Instant::now() < deadline {
                browser.poll();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(!browser.busy());
        }
        let before = fs::read(root.join(".git/index")).unwrap();
        let mut browser = Browser::default();
        browser.reload(root.clone());
        wait(&mut browser);
        browser.open_history("renamed.txt".into());
        assert_eq!(browser.active_tab, Some(0));
        assert!(browser.tabs[0].loading());
        browser.open_history("b.txt".into());
        assert_eq!(browser.active_tab, Some(1));
        browser.open_history("renamed.txt".into());
        assert_eq!(browser.tabs.len(), 2);
        assert_eq!(browser.active_tab, Some(0));
        wait(&mut browser);
        assert!(browser.tabs.iter().all(|tab| tab.error.is_none()));
        assert!(
            browser.tabs[0]
                .commits
                .iter()
                .any(|commit| commit.subject == "edit a")
        );
        assert_eq!(browser.tabs[1].commits.len(), 1);
        assert_eq!(browser.tabs[1].commits[0].subject, "base");
        browser.tabs[0].selected_hash = browser.tabs[0].commits.last().unwrap().hash.clone();
        browser.tabs[0].search = "base".into();
        let selected = browser.tabs[0].selected_hash.clone();
        assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
        fs::write(root.join("renamed.txt"), "new external change\n").unwrap();
        git(&["commit", "-am", "fresh commit after tab opened"]);
        let before = fs::read(root.join(".git/index")).unwrap();
        browser.activate(1);
        browser.activate(0);
        assert!(browser.tabs[0].loading());
        assert_eq!(browser.tabs[0].selected_hash, selected);
        assert_eq!(browser.tabs[0].search, "base");
        wait(&mut browser);
        assert_eq!(
            browser.tabs[0].commits[0].subject,
            "fresh commit after tab opened"
        );
        browser.open_history("untracked.txt".into());
        let cancelled = browser.tabs[2].task.as_ref().unwrap().cancelled.clone();
        browser.close(2);
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::sync::Arc::strong_count(&cancelled) > 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        wait(&mut browser);
        browser.close(0);
        assert_eq!(browser.selected.as_deref(), Some("b.txt"));
        browser.close(0);
        assert_eq!(browser.active_tab, None);
        assert_eq!(browser.selected, None);
        assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn hidden_file_toggle_filters_entire_hidden_directories() {
        let mut browser = Browser::default();
        browser.files = [
            ".claude/skills/a.md",
            "src/.internal/b.rs",
            "src/main.rs",
            ".env",
        ]
        .into_iter()
        .map(|path| git::WorktreeFile {
            path: path.into(),
            display_path: path.into(),
            ..Default::default()
        })
        .collect();
        browser.set_show_hidden(false);
        assert_eq!(browser.visible_files.len(), 1);
        assert_eq!(browser.visible_files[0].path, "src/main.rs");
        browser.set_show_hidden(true);
        assert_eq!(browser.visible_files.len(), 4);
        browser.set_show_hidden(false);
        assert_eq!(browser.visible_files.len(), 1);
    }

    #[test]
    fn filters_match_fuzzy_names_and_path_globs() {
        assert!(FileFilter::new("btn").matches("src/components/Button.vue"));
        assert!(FileFilter::new("COMPONENTS/*.VUE").matches("src/components/Button.vue"));
        assert!(FileFilter::new("components/*.vue").matches("components/Other.vue"));
        assert!(!FileFilter::new("components/*.vue").matches("src/components/forms/Input.vue"));
        assert!(!FileFilter::new("components/*.vue").matches("src/components/Button.ts"));
        assert!(FileFilter::new("components/**/*.vue").matches("src/components/forms/Input.vue"));
        assert!(FileFilter::new("components/**/*.vue").matches("src/components/Button.vue"));
        assert!(FileFilter::new("*.vue").matches("src/components/forms/Input.vue"));
        assert!(FileFilter::new("components\\*.vue").matches("src/components/Button.vue"));
        assert!(FileFilter::new("a?.vue").matches("src/Ab.vue"));
        assert!(!FileFilter::new("a?.vue").matches("src/Abc.vue"));
        assert!(FileFilter::new(" ").matches("any/file.txt"));
        assert!(FileFilter::new("组件/*.vue").matches("src/组件/按钮.vue"));
    }

    #[test]
    fn folders_default_closed_and_filter_preserves_manual_expansion() {
        let mut browser = Browser::default();
        browser.files = [
            "src/components/Button.vue",
            "src/components/forms/Input.vue",
            "docs/readme.md",
            ".hidden/Secret.vue",
        ]
        .into_iter()
        .map(|path| git::WorktreeFile {
            path: path.into(),
            display_path: path.into(),
            ..Default::default()
        })
        .collect();
        browser.initialize_directory_state();
        for directory in [
            "src",
            "src/components",
            "src/components/forms",
            "docs",
            ".hidden",
        ] {
            assert!(browser.collapsed.contains(directory));
        }
        browser.collapsed.remove("src");
        browser.initialize_directory_state();
        assert!(!browser.collapsed.contains("src"));
        assert!(browser.collapsed.contains("src/components"));
        let manual = browser.collapsed.clone();
        browser.set_filter("components/*.vue".into());
        assert_eq!(
            browser
                .visible_files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["src/components/Button.vue"]
        );
        assert!(browser.filter_collapsed.is_empty());
        browser.filter_collapsed.insert("src".into());
        browser.set_filter("*.vue".into());
        assert!(browser.filter_collapsed.is_empty());
        assert_eq!(browser.visible_files.len(), 2);
        browser.set_show_hidden(true);
        assert_eq!(browser.visible_files.len(), 3);
        browser.set_filter(String::new());
        assert_eq!(browser.collapsed, manual);
        browser.files.push(git::WorktreeFile {
            path: "src/new/deep/file.rs".into(),
            display_path: "src/new/deep/file.rs".into(),
            ..Default::default()
        });
        browser.initialize_directory_state();
        assert!(!browser.collapsed.contains("src"));
        assert!(browser.collapsed.contains("src/new"));
        assert!(browser.collapsed.contains("src/new/deep"));
    }

    #[test]
    fn reordering_tabs_preserves_active_file_and_query_ownership() {
        let mut browser = Browser::default();
        for path in ["a.txt", "b.txt", "c.txt"] {
            browser.tabs.push(HistoryTab {
                path: path.into(),
                commits: Vec::new(),
                selected_hash: path.into(),
                search: format!("search:{path}"),
                error: None,
                task: None,
            });
        }
        browser.active_tab = Some(1);
        browser.selected = Some("b.txt".into());
        let (sender, receiver) = mpsc::channel();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        browser.tabs[1].task = Some(HistoryTask {
            receiver,
            cancelled: cancelled.clone(),
        });
        browser.reorder(1, 0);
        assert_eq!(browser.active_tab, Some(0));
        assert_eq!(browser.selected.as_deref(), Some("b.txt"));
        assert_eq!(browser.tabs[0].search, "search:b.txt");
        assert!(!cancelled.load(std::sync::atomic::Ordering::Relaxed));
        sender.send(Ok(Vec::new())).unwrap();
        browser.poll();
        assert!(!browser.tabs[0].loading());
        browser.close(0);
        assert_eq!(browser.selected.as_deref(), Some("a.txt"));
    }
    #[test]
    fn history_task_disconnect_releases_loading_state() {
        let (sender, receiver) = mpsc::channel();
        let mut tab = HistoryTab {
            path: "a.txt".into(),
            commits: Vec::new(),
            selected_hash: String::new(),
            search: String::new(),
            error: None,
            task: Some(HistoryTask {
                receiver,
                cancelled: Default::default(),
            }),
        };
        assert!(tab.loading());
        drop(sender);
        tab.poll();
        assert!(!tab.loading());
        assert!(tab.error.as_ref().unwrap().contains("disconnected"));
    }
    #[test]
    fn file_listing_is_read_only_and_honors_ignore_rules() {
        let root = fixture("files");
        let git = |args: &[&str]| {
            let out = git::git_command()
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{:?}", out);
        };
        git(&["init"]);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("src/中文.rs"), "pub fn main() {}\n").unwrap();
        git(&["add", "."]);
        fs::write(root.join("untracked.txt"), "local\n").unwrap();
        fs::write(root.join("ignored.txt"), "ignored\n").unwrap();
        let before = fs::read(root.join(".git/index")).unwrap();
        let paths = list_files(&root).unwrap();
        assert!(paths.contains(&"src/中文.rs".to_owned()));
        assert!(paths.contains(&"untracked.txt".to_owned()));
        assert!(!paths.contains(&"ignored.txt".to_owned()));
        assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pending_tasks_drop_stale_results_and_release_busy_on_disconnect() {
        let mut browser = Browser::default();
        let (old_sender, old_receiver) = mpsc::channel();
        browser.list_task = Some(old_receiver);
        assert!(browser.busy());
        let (new_sender, new_receiver) = mpsc::channel();
        browser.list_task = Some(new_receiver);
        assert!(old_sender.send(Ok(vec!["stale".into()])).is_err());
        new_sender.send(Ok(vec!["fresh".into()])).unwrap();
        browser.poll();
        assert!(!browser.busy());
        assert_eq!(browser.files[0].path, "fresh");
    }
}
