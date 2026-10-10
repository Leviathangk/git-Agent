//! Versioned, ordered feature guides. Add a Guide to GUIDES to introduce a new tour.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
};

pub const BASIC_ID: &str = "basics";
pub const PLUGINS_ID: &str = "plugins";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    MainMenus,
    RepositoryTabs,
    RepositoryToolbar,
    SidebarNavigation,
    RepositoryResources,
    WorkspaceChanges,
    CommitArea,
    GlobalTools,
    PluginManagement,
    RepositoryPluginManagement,
}

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub target: Target,
    pub title_key: &'static str,
    pub body_key: &'static str,
}

pub const BASIC_STEPS: [Step; 10] = [
    Step {
        target: Target::MainMenus,
        title_key: "tutorial.menus.title",
        body_key: "tutorial.menus.body",
    },
    Step {
        target: Target::RepositoryTabs,
        title_key: "tutorial.repositories.title",
        body_key: "tutorial.repositories.body",
    },
    Step {
        target: Target::RepositoryToolbar,
        title_key: "tutorial.git_actions.title",
        body_key: "tutorial.git_actions.body",
    },
    Step {
        target: Target::SidebarNavigation,
        title_key: "tutorial.navigation.title",
        body_key: "tutorial.navigation.body",
    },
    Step {
        target: Target::RepositoryResources,
        title_key: "tutorial.resources.title",
        body_key: "tutorial.resources.body",
    },
    Step {
        target: Target::WorkspaceChanges,
        title_key: "tutorial.changes.title",
        body_key: "tutorial.changes.body",
    },
    Step {
        target: Target::WorkspaceChanges,
        title_key: "tutorial.worktree_shortcuts.title",
        body_key: "tutorial.worktree_shortcuts.body",
    },
    Step {
        target: Target::CommitArea,
        title_key: "tutorial.commit.title",
        body_key: "tutorial.commit.body",
    },
    Step {
        target: Target::CommitArea,
        title_key: "tutorial.commit_shortcuts.title",
        body_key: "tutorial.commit_shortcuts.body",
    },
    Step {
        target: Target::GlobalTools,
        title_key: "tutorial.tools.title",
        body_key: "tutorial.tools.body",
    },
];
const PLUGIN_STEPS: [Step; 4] = [
    Step {
        target: Target::PluginManagement,
        title_key: "tutorial.plugins.install.title",
        body_key: "tutorial.plugins.install.body",
    },
    Step {
        target: Target::PluginManagement,
        title_key: "tutorial.plugins.manage.title",
        body_key: "tutorial.plugins.manage.body",
    },
    Step {
        target: Target::RepositoryPluginManagement,
        title_key: "tutorial.plugins.repository.title",
        body_key: "tutorial.plugins.repository.body",
    },
    Step {
        target: Target::RepositoryToolbar,
        title_key: "tutorial.plugins.pages.title",
        body_key: "tutorial.plugins.pages.body",
    },
];

#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub id: &'static str,
    pub revision: u32,
    pub title_key: &'static str,
    pub steps: &'static [Step],
}

pub static GUIDES: [Guide; 2] = [
    Guide {
        id: BASIC_ID,
        revision: 1,
        title_key: "tutorial.basics",
        steps: &BASIC_STEPS,
    },
    Guide {
        id: PLUGINS_ID,
        revision: 1,
        title_key: "tutorial.plugins",
        steps: &PLUGIN_STEPS,
    },
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Progress {
    pub versions: BTreeMap<String, u32>,
}

pub struct Session {
    pub progress: Progress,
    pending: VecDeque<&'static Guide>,
    step: usize,
    paused: bool,
}

impl Session {
    pub fn automatic(progress: Progress, legacy_seen: bool) -> Self {
        Self::with_registry(progress, legacy_seen, &GUIDES)
    }
    fn with_registry(mut progress: Progress, legacy_seen: bool, guides: &'static [Guide]) -> Self {
        if legacy_seen {
            progress.versions.entry(BASIC_ID.into()).or_insert(1);
        }
        let pending = guides
            .iter()
            .filter(|guide| {
                !guide.steps.is_empty()
                    && progress.versions.get(guide.id).copied().unwrap_or(0) < guide.revision
            })
            .collect();
        Self {
            progress,
            pending,
            step: 0,
            paused: false,
        }
    }
    pub fn current(&self) -> Option<(&'static Guide, usize, Step)> {
        if self.paused {
            return None;
        }
        let guide = *self.pending.front()?;
        Some((guide, self.step, *guide.steps.get(self.step)?))
    }
    pub fn seen_basics(&self) -> bool {
        self.progress.versions.get(BASIC_ID).copied().unwrap_or(0) >= 1
    }
    pub fn has_next_guide(&self) -> bool {
        self.pending.len() > 1
    }
    pub fn previous(&mut self) {
        self.step = self.step.saturating_sub(1);
    }
    /// True when a guide ended and the progress snapshot needs persistence.
    pub fn next(&mut self) -> bool {
        let Some(guide) = self.pending.front() else {
            return false;
        };
        if self.step + 1 < guide.steps.len() {
            self.step += 1;
            false
        } else {
            self.skip();
            true
        }
    }
    /// Dismissing a guide consumes only that guide; the next unseen guide follows.
    pub fn skip(&mut self) {
        if let Some(guide) = self.pending.pop_front() {
            let version = self.progress.versions.entry(guide.id.into()).or_default();
            *version = (*version).max(guide.revision);
        }
        self.step = 0;
        self.paused = false;
    }
    pub fn replay(&mut self, id: Option<&str>) {
        self.pending = GUIDES
            .iter()
            .filter(|guide| id.is_none_or(|id| id == guide.id))
            .collect();
        self.step = 0;
        self.paused = false;
    }
    pub fn pause(&mut self) {
        self.paused = true;
    }
    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn resume(&mut self) {
        self.paused = false;
    }
}

pub fn load(path: &Path) -> Result<Progress, String> {
    match fs::read(path) {
        Ok(bytes) if bytes.len() <= 1024 * 1024 => {
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())
        }
        Ok(_) => Err("Tutorial progress exceeds 1 MiB".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Progress::default()),
        Err(error) => Err(error.to_string()),
    }
}

struct WriteTask {
    receiver: Receiver<Result<(), String>>,
    worker: Option<JoinHandle<()>>,
}
pub struct Persistence {
    path: PathBuf,
    pending: Option<WriteTask>,
    queued: Option<Progress>,
}
impl Persistence {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            pending: None,
            queued: None,
        }
    }
    pub fn request(&mut self, progress: &Progress) {
        if self.pending.is_some() {
            self.queued = Some(progress.clone());
            return;
        }
        let path = self.path.clone();
        let snapshot = progress.clone();
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(WriteTask {
            receiver,
            worker: None,
        });
        let worker = thread::Builder::new()
            .name("tutorial-progress".into())
            .spawn(move || {
                let _ = sender.send(write(&path, &snapshot));
            })
            .ok();
        self.pending.as_mut().unwrap().worker = worker;
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        let task = self.pending.take()?;
        let result = match task.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                self.pending = Some(task);
                return None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Tutorial progress worker disconnected".into())
            }
        };
        if let Some(worker) = task.worker {
            let _ = worker.join();
        }
        if let Some(next) = self.queued.take() {
            self.request(&next);
        }
        Some(result)
    }
    /// Flush after the window closes, so a fast exit cannot discard a skip/completion.
    pub fn finish(&mut self, latest: &Progress) -> Result<(), String> {
        if let Some(task) = self.pending.take() {
            if let Some(worker) = task.worker {
                let _ = worker.join();
            }
        }
        self.queued = None;
        write(&self.path, latest)
    }
}
fn write(path: &Path, progress: &Progress) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or("Missing tutorial progress directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut merged = load(path).unwrap_or_default();
    for (id, revision) in &progress.versions {
        let stored = merged.versions.entry(id.clone()).or_default();
        *stored = (*stored).max(*revision);
    }
    let temporary = parent.join(format!(
        "tutorial-progress-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
        file.write_all(&serde_json::to_vec(&merged).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if temporary.exists() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_guides_have_stable_unique_ids_and_complete_bilingual_copy() {
        let mut ids = std::collections::BTreeSet::new();
        for guide in &GUIDES {
            assert!(ids.insert(guide.id));
            assert!(guide.revision > 0 && !guide.steps.is_empty());
            for language in [
                crate::i18n::Language::Chinese,
                crate::i18n::Language::English,
            ] {
                assert_ne!(crate::i18n::t(language, guide.title_key), guide.title_key);
                for step in guide.steps {
                    assert_ne!(crate::i18n::t(language, step.title_key), step.title_key);
                    assert_ne!(crate::i18n::t(language, step.body_key), step.body_key);
                }
            }
        }
    }
    #[test]
    fn new_users_receive_both_guides_in_order_and_skip_is_per_guide() {
        let mut session = Session::automatic(Progress::default(), false);
        assert_eq!(session.current().unwrap().0.id, BASIC_ID);
        session.skip();
        assert_eq!(session.current().unwrap().0.id, PLUGINS_ID);
        session.skip();
        assert!(
            Session::automatic(session.progress, false)
                .current()
                .is_none()
        );
    }
    #[test]
    fn legacy_seen_users_receive_only_plugins_and_completion_survives_restart() {
        let mut session = Session::automatic(Progress::default(), true);
        assert_eq!(session.current().unwrap().0.id, PLUGINS_ID);
        for _ in 0..PLUGIN_STEPS.len() - 1 {
            assert!(!session.next());
        }
        assert!(session.next());
        assert!(
            Session::automatic(session.progress, true)
                .current()
                .is_none()
        );
    }
    #[test]
    fn completing_basics_chains_plugins_and_unfinished_plugins_resume() {
        let mut session = Session::automatic(Progress::default(), false);
        for index in 0..BASIC_STEPS.len() {
            assert_eq!(session.current().unwrap().1, index);
            assert_eq!(session.next(), index + 1 == BASIC_STEPS.len());
        }
        assert_eq!(session.current().unwrap().0.id, PLUGINS_ID);
        let restarted = Session::automatic(session.progress.clone(), session.seen_basics());
        assert_eq!(restarted.current().unwrap().0.id, PLUGINS_ID);
        assert_eq!(restarted.current().unwrap().1, 0);
    }
    #[test]
    fn revision_updates_and_new_guides_do_not_replay_old_guides_or_drop_unknown_progress() {
        static FUTURE: [Guide; 3] = [
            Guide {
                id: BASIC_ID,
                revision: 1,
                title_key: "tutorial.basics",
                steps: &BASIC_STEPS,
            },
            Guide {
                id: PLUGINS_ID,
                revision: 2,
                title_key: "tutorial.plugins",
                steps: &PLUGIN_STEPS,
            },
            Guide {
                id: "future",
                revision: 1,
                title_key: "future",
                steps: &PLUGIN_STEPS,
            },
        ];
        let progress = Progress {
            versions: BTreeMap::from([
                (BASIC_ID.into(), 1),
                (PLUGINS_ID.into(), 1),
                ("other".into(), 8),
            ]),
        };
        let mut session = Session::with_registry(progress, true, &FUTURE);
        assert_eq!(session.current().unwrap().0.id, PLUGINS_ID);
        session.skip();
        assert_eq!(session.current().unwrap().0.id, "future");
        assert_eq!(session.progress.versions["other"], 8);
        session.skip();
        assert!(
            Session::with_registry(session.progress, true, &GUIDES)
                .current()
                .is_none()
        );
    }
    #[test]
    fn font_settings_pause_keeps_the_next_guide_and_replay_is_explicit() {
        let mut session = Session::automatic(Progress::default(), false);
        session.skip();
        session.pause();
        assert!(session.current().is_none());
        session.resume();
        assert_eq!(session.current().unwrap().0.id, PLUGINS_ID);
        session.skip();
        session.replay(Some(PLUGINS_ID));
        assert_eq!(session.current().unwrap().0.id, PLUGINS_ID);
        assert_eq!(session.progress.versions[PLUGINS_ID], 1);
    }
    #[test]
    fn async_progress_writes_coalesce_and_fast_exit_flushes_the_latest_snapshot() {
        let dir = std::env::temp_dir().join(format!(
            "git-agent-tutorial-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("tutorial-progress.json");
        let mut writer = Persistence::new(path.clone());
        let mut session = Session::automatic(Progress::default(), false);
        writer.request(&session.progress);
        assert!(writer.busy());
        session.skip();
        writer.request(&session.progress);
        session.skip();
        writer.request(&session.progress);
        writer.finish(&session.progress).unwrap();
        assert!(!writer.busy());
        let stored = load(&path).unwrap();
        assert!(
            Session::automatic(stored.clone(), false)
                .current()
                .is_none()
        );
        assert_eq!(stored, session.progress);
    }
    #[test]
    fn failed_progress_worker_releases_ownership_and_reports_the_error() {
        let path =
            std::env::temp_dir().join(format!("git-agent-tutorial-blocked-{}", std::process::id()));
        fs::write(&path, b"not a directory").unwrap();
        let mut writer = Persistence::new(path.join("progress.json"));
        writer.request(&Progress::default());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = writer.poll() {
                assert!(result.is_err());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(!writer.busy());
    }
    #[test]
    fn stale_snapshots_preserve_completed_revisions_already_saved_on_disk() {
        let path = std::env::temp_dir().join(format!(
            "git-agent-tutorial-monotonic-{}/progress.json",
            std::process::id()
        ));
        let latest = Progress {
            versions: BTreeMap::from([(PLUGINS_ID.into(), 2), ("future".into(), 3)]),
        };
        write(&path, &latest).unwrap();
        write(
            &path,
            &Progress {
                versions: BTreeMap::from([(BASIC_ID.into(), 1), (PLUGINS_ID.into(), 1)]),
            },
        )
        .unwrap();
        let stored = load(&path).unwrap();
        assert_eq!(stored.versions[PLUGINS_ID], 2);
        assert_eq!(stored.versions["future"], 3);
        assert_eq!(stored.versions[BASIC_ID], 1);
    }
}
