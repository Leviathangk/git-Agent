//! Package and preference operations do not load native code. All UI mutations run on workers.
use crate::plugins::{Extension, inspect_plugin};
use eframe::egui;
use git_agent_plugin_sdk::Manifest;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);
const MAX_PACKAGE: u64 = 32 * 1024 * 1024;
const MAX_CONTENTS: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    #[serde(default)]
    pub pending_version: Option<String>,
    #[serde(default)]
    pub remove_on_restart: bool,
    #[serde(default)]
    pub last_error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    #[serde(default)]
    pub bundled_seen: BTreeSet<String>,
    #[serde(default)]
    pub repositories: BTreeMap<String, BTreeMap<String, bool>>,
}
impl Registry {
    pub fn enabled(&self, id: &str, root: &str) -> bool {
        !self.disabled.contains(id)
            && !self.installed.get(id).is_some_and(|p| p.remove_on_restart)
            && self
                .repositories
                .get(&repository_key(root))
                .and_then(|p| p.get(id))
                .copied()
                .unwrap_or(true)
    }
}
fn repository_key(root: &str) -> String {
    let root = root.replace('\\', "/").trim_end_matches('/').to_owned();
    if cfg!(windows) {
        root.to_lowercase()
    } else {
        root
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && ![".", ".."].contains(&id)
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}
fn unique_name(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Clone)]
pub struct Storage {
    pub root: PathBuf,
}
pub fn default_storage() -> Storage {
    Storage::new(
        crate::app::app_data_dir()
            .unwrap_or_else(|| std::env::temp_dir().join("git-agent-data"))
            .join("plugins"),
    )
}
impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn load(&self) -> Result<Registry, String> {
        let path = self.root.join("registry.json");
        if !path.exists() {
            return Ok(Registry::default());
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Err("Plugin registry is too large".into());
        }
        let registry: Registry =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid plugin registry: {e}"))?;
        for (id, installed) in &registry.installed {
            if !valid_id(id)
                || semver::Version::parse(&installed.version).is_err()
                || installed
                    .pending_version
                    .as_ref()
                    .is_some_and(|v| semver::Version::parse(v).is_err())
            {
                return Err("Invalid installed plugin identity/version".into());
            }
        }
        Ok(registry)
    }
    pub fn save(&self, registry: &Registry) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let temp = self.root.join(unique_name("registry.tmp"));
        let bytes = serde_json::to_vec_pretty(registry).map_err(|e| e.to_string())?;
        let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temp, self.root.join("registry.json")).map_err(|e| e.to_string())
    }
    pub fn directory(&self, id: &str, version: &str) -> PathBuf {
        self.root.join("packages").join(id).join(version)
    }
    fn remove_owned(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let base = self.root.canonicalize().map_err(|e| e.to_string())?;
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        if path == base || !path.starts_with(&base) {
            return Err("Plugin cleanup path escaped storage".into());
        }
        fs::remove_dir_all(path).map_err(|e| e.to_string())
    }
    fn prepare_owned_directory(&self, directory: &Path) -> Result<(), String> {
        let base = self.root.canonicalize().map_err(|e| e.to_string())?;
        let relative = directory
            .strip_prefix(&self.root)
            .map_err(|_| "Package directory escaped storage")?;
        let mut current = base.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err("Invalid package directory".into());
            };
            let next = current.join(name);
            if !next.exists() {
                fs::create_dir(&next).map_err(|e| e.to_string())?;
            }
            current = next.canonicalize().map_err(|e| e.to_string())?;
            if !current.starts_with(&base) {
                return Err("Package directory link escaped storage".into());
            }
        }
        Ok(())
    }
    /// Import installer-provided packages before any native library loads. Preserve user choices.
    pub fn seed_bundled(&self, directory: &Path) -> Result<Vec<String>, String> {
        let mut errors = Vec::new();
        if !directory.exists() {
            return Ok(errors);
        }
        for file in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = file.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("gaplugin") {
                continue;
            }
            let result: Result<(), String> = (|| {
                let mut archive =
                    zip::ZipArchive::new(fs::File::open(&path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                archive
                    .by_name("package.json")
                    .map_err(|e| e.to_string())?
                    .take(65537)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 65536 {
                    return Err("Bundled plugin metadata too large".into());
                }
                let info: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let id = info["id"].as_str().ok_or("Missing bundled plugin ID")?;
                let version = info["version"]
                    .as_str()
                    .ok_or("Missing bundled plugin version")?;
                let target = info["target"]
                    .as_str()
                    .ok_or("Missing bundled plugin target")?;
                let matching_os = if cfg!(windows) {
                    target.contains("-windows-")
                } else if cfg!(target_os = "macos") {
                    target.ends_with("-apple-darwin")
                } else {
                    target.contains("-linux-")
                };
                if !target.starts_with(&format!("{}-", std::env::consts::ARCH)) || !matching_os {
                    return Ok(());
                }
                if !valid_id(id) {
                    return Err("Invalid bundled plugin ID".into());
                }
                let version = semver::Version::parse(version).map_err(|e| e.to_string())?;
                let mut registry = self.load()?;
                let install = if let Some(current) = registry.installed.get(id) {
                    !current.remove_on_restart
                        && version
                            > semver::Version::parse(
                                current.pending_version.as_ref().unwrap_or(&current.version),
                            )
                            .map_err(|e| e.to_string())?
                } else {
                    !registry.bundled_seen.contains(id)
                };
                if install {
                    registry = self.install(&path)?;
                }
                registry.bundled_seen.insert(id.into());
                self.save(&registry)?;
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(format!("{}: {error}", path.display()));
            }
        }
        Ok(errors)
    }
    pub fn activate_pending(&self) -> Result<Registry, String> {
        let mut registry = self.load()?;
        let mut changed = false;
        let ids = registry.installed.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let installed = registry.installed.get_mut(&id).unwrap();
            if installed.remove_on_restart {
                match self.remove_owned(&self.root.join("packages").join(&id)) {
                    Ok(()) => {
                        registry.installed.remove(&id);
                        changed = true;
                    }
                    Err(error) => {
                        installed.last_error = Some(error);
                        changed = true;
                    }
                }
            } else if let Some(version) = installed.pending_version.clone() {
                let validation =
                    inspect_plugin(&self.directory(&id, &version)).and_then(|(manifest, _, _)| {
                        if manifest.id != id || manifest.version != version {
                            return Err(
                                "Pending plugin identity/version does not match registry".into()
                            );
                        }
                        Ok(())
                    });
                match validation {
                    Ok(()) => {
                        installed.version = version;
                        installed.pending_version = None;
                        installed.last_error = None;
                    }
                    Err(error) => {
                        installed.last_error = Some(error);
                    }
                }
                changed = true;
            }
        }
        if changed {
            self.save(&registry)?;
        }
        Ok(registry)
    }
    pub fn install(&self, archive: &Path) -> Result<Registry, String> {
        let mut registry = self.load()?;
        if fs::metadata(archive).map_err(|e| e.to_string())?.len() > MAX_PACKAGE {
            return Err("Plugin package exceeds 32 MiB".into());
        }
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let staging = self.root.join(unique_name("staging"));
        fs::create_dir(&staging).map_err(|e| e.to_string())?;
        let result = (|| {
            let mut raw = Vec::new();
            fs::File::open(archive)
                .map_err(|e| e.to_string())?
                .take(MAX_PACKAGE + 1)
                .read_to_end(&mut raw)
                .map_err(|e| e.to_string())?;
            if raw.len() as u64 > MAX_PACKAGE {
                return Err("Plugin package exceeds 32 MiB".into());
            }
            let footer = raw
                .windows(4)
                .rposition(|bytes| bytes == b"PK\x05\x06")
                .ok_or("Missing ZIP directory footer")?;
            let declared = u16::from_le_bytes(
                raw.get(footer + 10..footer + 12)
                    .ok_or("Invalid ZIP footer")?
                    .try_into()
                    .unwrap(),
            ) as usize;
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(raw))
                .map_err(|e| format!("Invalid plugin ZIP: {e}"))?;
            if declared != zip.len() {
                return Err("Duplicate ZIP entry or unsupported ZIP64 package".into());
            }
            if zip.len() > 512 {
                return Err("Plugin package has too many files".into());
            }
            let mut total = 0u64;
            let mut names = HashSet::new();
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
                let name = entry.name().to_owned();
                let path = Path::new(&name);
                if name.is_empty()
                    || name
                        .split('/')
                        .any(|part| part == "." || part == ".." || part.ends_with(['.', ' ']))
                    || name
                        .split('/')
                        .any(|part| part.is_empty() && !entry.is_dir())
                    || name.contains(['\\', ':'])
                    || path.is_absolute()
                    || path
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_)))
                    || !names.insert(if cfg!(windows) {
                        name.to_lowercase()
                    } else {
                        name.clone()
                    })
                    || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
                {
                    return Err("Unsafe or duplicate plugin archive path".into());
                }
                total = total
                    .checked_add(entry.size())
                    .ok_or("Plugin size overflow")?;
                if total > MAX_CONTENTS {
                    return Err("Plugin unpacked contents exceed 64 MiB".into());
                }
                let target = staging.join(path);
                if entry.is_dir() {
                    fs::create_dir_all(target).map_err(|e| e.to_string())?;
                    continue;
                }
                fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                let mut output = fs::File::create(target).map_err(|e| e.to_string())?;
                let count = std::io::copy(&mut entry.by_ref().take(MAX_CONTENTS + 1), &mut output)
                    .map_err(|e| e.to_string())?;
                if count != entry.size() {
                    return Err("Plugin entry length mismatch".into());
                }
            }
            let (manifest, _, _) = inspect_plugin(&staging)?;
            let version = semver::Version::parse(&manifest.version)
                .map_err(|_| "Invalid plugin semantic version")?;
            let package: serde_json::Value = serde_json::from_slice(
                &fs::read(staging.join("package.json")).map_err(|_| "Missing package.json")?,
            )
            .map_err(|e| e.to_string())?;
            if package["format"] != 1
                || package["api_version"] != git_agent_plugin_sdk::API_VERSION
                || package["id"] != manifest.id
                || package["version"] != manifest.version
            {
                return Err("Plugin package metadata does not match manifest/API".into());
            }
            let target = package["target"].as_str().ok_or("Missing package target")?;
            let os = std::env::consts::OS;
            let os_match = match os {
                "windows" => target.contains("-windows-"),
                "macos" => target.ends_with("-apple-darwin"),
                "linux" => target.contains("-linux-"),
                _ => false,
            };
            if !target.starts_with(&format!("{}-", std::env::consts::ARCH)) || !os_match {
                return Err("Plugin package platform/architecture does not match host".into());
            }
            if let Some(min) = package["min_host_version"].as_str() {
                if semver::Version::parse(min).map_err(|_| "Invalid minimum host version")?
                    > semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap()
                {
                    return Err("Plugin requires a newer Git Agent".into());
                }
            }
            let platform = if os == "macos" { "macos" } else { os };
            validate_binary(
                &fs::read(staging.join(&manifest.library[platform])).map_err(|e| e.to_string())?,
            )?;
            if let Some(old) = registry.installed.get(&manifest.id) {
                let latest = old.pending_version.as_ref().unwrap_or(&old.version);
                if version <= semver::Version::parse(latest).unwrap() {
                    return Err("Install a newer version to update an existing plugin".into());
                }
                if old.remove_on_restart {
                    return Err("Plugin uninstall is pending; restart before reinstalling".into());
                }
            }
            let directory = self.directory(&manifest.id, &manifest.version);
            if directory.exists() {
                return Err("Plugin version directory already exists".into());
            }
            self.prepare_owned_directory(directory.parent().unwrap())?;
            fs::rename(&staging, &directory).map_err(|e| e.to_string())?;
            if let Some(old) = registry.installed.get_mut(&manifest.id) {
                old.pending_version = Some(manifest.version.clone());
            } else {
                registry.installed.insert(
                    manifest.id.clone(),
                    Installed {
                        version: manifest.version.clone(),
                        ..Default::default()
                    },
                );
            }
            if let Err(error) = self.save(&registry) {
                self.remove_owned(&directory)?;
                return Err(error);
            }
            Ok(registry)
        })();
        if staging.exists() {
            self.remove_owned(&staging)?;
        }
        result
    }
    pub fn set_global(&self, id: &str, enabled: bool) -> Result<Registry, String> {
        if !valid_id(id) {
            return Err("Invalid plugin ID".into());
        }
        let mut registry = self.load()?;
        if enabled {
            registry.disabled.remove(id);
        } else {
            registry.disabled.insert(id.into());
        }
        self.save(&registry)?;
        Ok(registry)
    }
    pub fn set_repository(
        &self,
        root: &str,
        id: &str,
        enabled: Option<bool>,
    ) -> Result<Registry, String> {
        if !valid_id(id) || root.is_empty() {
            return Err("Invalid repository/plugin identity".into());
        }
        let mut registry = self.load()?;
        let settings = registry
            .repositories
            .entry(repository_key(root))
            .or_default();
        if let Some(enabled) = enabled {
            settings.insert(id.into(), enabled);
        } else {
            settings.remove(id);
        }
        registry
            .repositories
            .retain(|_, settings| !settings.is_empty());
        self.save(&registry)?;
        Ok(registry)
    }
    pub fn uninstall(&self, id: &str) -> Result<Registry, String> {
        let mut registry = self.load()?;
        let installed = registry
            .installed
            .get_mut(id)
            .ok_or("Development/manual plugins must be removed from their source directory")?;
        installed.remove_on_restart = true;
        registry.disabled.insert(id.into());
        self.save(&registry)?;
        Ok(registry)
    }
}

fn validate_binary(bytes: &[u8]) -> Result<(), String> {
    let architecture = if cfg!(windows) {
        if !bytes.starts_with(b"MZ") {
            return Err("Expected a Windows DLL".into());
        }
        let offset = bytes.get(60..64).ok_or("Invalid PE header")?;
        let offset = u32::from_le_bytes(offset.try_into().unwrap()) as usize;
        if bytes.get(offset..offset + 4) != Some(&b"PE\0\0"[..]) {
            return Err("Invalid PE signature".into());
        }
        match u16::from_le_bytes(
            bytes
                .get(offset + 4..offset + 6)
                .ok_or("Invalid PE machine")?
                .try_into()
                .unwrap(),
        ) {
            0x8664 => "x86_64",
            0xaa64 => "aarch64",
            0x14c => "x86",
            _ => "unknown",
        }
    } else if cfg!(target_os = "linux") {
        if !bytes.starts_with(b"\x7fELF") || bytes.get(5) != Some(&1) {
            return Err("Expected a little-endian ELF shared library".into());
        }
        match u16::from_le_bytes(
            bytes
                .get(18..20)
                .ok_or("Invalid ELF machine")?
                .try_into()
                .unwrap(),
        ) {
            62 => "x86_64",
            183 => "aarch64",
            3 => "x86",
            _ => "unknown",
        }
    } else {
        if bytes.get(0..4) != Some(&[0xcf, 0xfa, 0xed, 0xfe][..]) {
            return Err("Expected a 64-bit Mach-O shared library".into());
        }
        match u32::from_le_bytes(
            bytes
                .get(4..8)
                .ok_or("Invalid Mach-O CPU")?
                .try_into()
                .unwrap(),
        ) {
            0x01000007 => "x86_64",
            0x0100000c => "aarch64",
            _ => "unknown",
        }
    };
    if architecture != std::env::consts::ARCH {
        return Err("Native library architecture does not match host".into());
    }
    Ok(())
}

#[derive(Clone)]
pub struct CatalogEntry {
    pub id: String,
    pub path: PathBuf,
    pub manifest: Option<Manifest>,
    pub error: String,
    pub development: bool,
    pub managed: bool,
    pub missing_tools: Vec<String>,
}
#[derive(Clone, Default)]
pub struct Catalog {
    pub registry: Registry,
    pub entries: Vec<CatalogEntry>,
    pub warnings: Vec<String>,
}
fn command_available(command: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions = if cfg!(windows) {
        vec!["", ".exe", ".com", ".cmd", ".bat"]
    } else {
        vec![""]
    };
    std::env::split_paths(&paths).any(|directory| {
        extensions
            .iter()
            .any(|extension| directory.join(format!("{command}{extension}")).is_file())
    })
}
pub fn catalog(
    storage: &Storage,
    directories: &[PathBuf],
    startup: bool,
) -> Result<Catalog, String> {
    let warnings = if startup {
        storage.seed_bundled(
            &std::env::current_exe()
                .map_err(|e| e.to_string())?
                .parent()
                .ok_or("Missing application directory")?
                .join("bundled-plugins"),
        )?
    } else {
        Vec::new()
    };
    let registry = if startup {
        storage.activate_pending()?
    } else {
        storage.load()?
    };
    let mut candidates = Vec::new();
    for (index, directory) in directories.iter().enumerate() {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.join("plugin.json").is_file() {
                candidates.push((
                    path,
                    index == 0 && std::env::var_os("GIT_AGENT_PLUGIN_DIR").is_some(),
                    false,
                ));
            }
        }
    }
    for (id, installed) in &registry.installed {
        candidates.push((storage.directory(id, &installed.version), false, true));
    }
    candidates.sort_by_key(|(_, development, managed)| {
        if *development {
            0
        } else if *managed {
            1
        } else {
            2
        }
    });
    let mut entries: Vec<CatalogEntry> = Vec::new();
    for (path, development, managed) in candidates {
        let mut parsed = inspect_plugin(&path);
        let fallback = if managed {
            path.parent().and_then(|p| p.file_name())
        } else {
            path.file_name()
        }
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
        if managed
            && parsed
                .as_ref()
                .is_ok_and(|(manifest, _, _)| manifest.id != fallback)
        {
            parsed = Err("Managed plugin identity does not match registry".into());
        }
        let id = parsed
            .as_ref()
            .map(|(m, _, _)| m.id.clone())
            .unwrap_or(fallback);
        if let Some(existing) = entries.iter_mut().find(|entry| entry.id == id) {
            if existing.path.parent() == path.parent() && existing.managed == managed {
                existing.error = format!("Duplicate plugin ID: {id}");
            }
            continue;
        }
        let (manifest, error) = match parsed {
            Ok((m, _, _)) => (Some(m), String::new()),
            Err(error) => (None, error),
        };
        let missing_tools = manifest
            .as_ref()
            .map(|m| {
                m.required_tools
                    .iter()
                    .filter(|tool| !command_available(tool))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        entries.push(CatalogEntry {
            missing_tools,
            id,
            path,
            manifest,
            error,
            development,
            managed,
        });
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Catalog {
        registry,
        entries,
        warnings,
    })
}
pub fn load_enabled(catalog: &Catalog) -> (Vec<Arc<Extension>>, Vec<String>) {
    load_enabled_reusing(catalog, &std::collections::HashMap::new())
}
pub(crate) fn load_enabled_reusing(
    catalog: &Catalog,
    cache: &std::collections::HashMap<String, Arc<Extension>>,
) -> (Vec<Arc<Extension>>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = catalog.warnings.clone();
    for entry in &catalog.entries {
        if !entry.error.is_empty() {
            errors.push(format!("{}: {}", entry.id, entry.error));
            continue;
        }
        if !catalog.registry.enabled(&entry.id, "") {
            continue;
        }
        if let Some(plugin) = cache.get(&entry.id) {
            plugins.push(plugin.clone());
            continue;
        }
        match Extension::load(&entry.path) {
            Ok(plugin) => plugins.push(plugin),
            Err(error) => errors.push(format!("{}: {error}", entry.id)),
        }
    }
    (plugins, errors)
}

#[derive(Clone)]
pub enum Operation {
    Refresh,
    Install(PathBuf),
    PickInstall,
    Global(String, bool),
    Repository(String, String, Option<bool>),
    Uninstall(String),
}
impl Operation {
    pub fn blocks(&self, id: &str, root: &str) -> bool {
        match self {
            Self::Global(plugin, false) | Self::Uninstall(plugin) => plugin == id,
            Self::Repository(repo, plugin, Some(false)) => {
                plugin == id && repository_key(repo) == repository_key(root)
            }
            _ => false,
        }
    }
}
struct Pending {
    operation: Operation,
    receiver: Receiver<Result<Catalog, String>>,
}
pub struct Manager {
    pub storage: Storage,
    pub directories: Vec<PathBuf>,
    pub catalog: Catalog,
    pub ready: bool,
    pub error: String,
    pub notice: String,
    pending: Option<Pending>,
}
impl Manager {
    pub fn idle(storage: Storage, directories: Vec<PathBuf>) -> Self {
        Self {
            storage,
            directories,
            catalog: Catalog::default(),
            ready: false,
            error: String::new(),
            notice: String::new(),
            pending: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn operation(&self) -> Option<&Operation> {
        self.pending.as_ref().map(|p| &p.operation)
    }
    pub fn enabled(&self, id: &str, root: &str) -> bool {
        self.ready
            && !self
                .pending
                .as_ref()
                .is_some_and(|p| p.operation.blocks(id, root))
            && self.catalog.registry.enabled(id, root)
    }
    pub fn start(&mut self, operation: Operation, startup: bool, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(Pending {
            operation: operation.clone(),
            receiver,
        });
        self.error.clear();
        self.notice.clear();
        ctx.request_repaint();
        let storage = self.storage.clone();
        let directories = self.directories.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                match operation {
                    Operation::Refresh => {}
                    Operation::Install(path) => {
                        storage.install(&path)?;
                    }
                    Operation::PickInstall => {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Git Agent plugins", &["gaplugin", "zip"])
                            .pick_file()
                        {
                            storage.install(&path)?;
                        }
                    }
                    Operation::Global(id, enabled) => {
                        storage.set_global(&id, enabled)?;
                    }
                    Operation::Repository(root, id, enabled) => {
                        storage.set_repository(&root, &id, enabled)?;
                    }
                    Operation::Uninstall(id) => {
                        storage.uninstall(&id)?;
                    }
                }
                catalog(&storage, &directories, startup)
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        match pending.receiver.try_recv() {
            Ok(Ok(catalog)) => {
                self.catalog = catalog;
                self.ready = true;
                ctx.request_repaint();
                true
            }
            Ok(Err(error)) => {
                self.error = error;
                ctx.request_repaint();
                false
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pending = Some(pending);
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
                false
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = "Plugin manager worker disconnected".into();
                ctx.request_repaint();
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zip::write::SimpleFileOptions;
    fn storage() -> Storage {
        let root = std::env::temp_dir().join(unique_name("git-agent-plugin-manager-test"));
        fs::create_dir_all(&root).unwrap();
        Storage::new(root)
    }
    fn platform() -> &'static str {
        if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
    }
    fn target() -> String {
        format!(
            "{}-{}",
            std::env::consts::ARCH,
            if cfg!(windows) {
                "pc-windows-msvc"
            } else if cfg!(target_os = "macos") {
                "apple-darwin"
            } else {
                "unknown-linux-gnu"
            }
        )
    }
    fn binary() -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        if cfg!(windows) {
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
            bytes[68..70].copy_from_slice(
                &(if std::env::consts::ARCH == "aarch64" {
                    0xaa64u16
                } else {
                    0x8664
                })
                .to_le_bytes(),
            );
        } else if cfg!(target_os = "linux") {
            bytes[..4].copy_from_slice(b"\x7fELF");
            bytes[4] = 2;
            bytes[5] = 1;
            bytes[18..20].copy_from_slice(
                &(if std::env::consts::ARCH == "aarch64" {
                    183u16
                } else {
                    62
                })
                .to_le_bytes(),
            );
        } else {
            bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
            bytes[4..8].copy_from_slice(
                &(if std::env::consts::ARCH == "aarch64" {
                    0x0100000cu32
                } else {
                    0x01000007
                })
                .to_le_bytes(),
            );
        }
        bytes
    }
    fn package_values(version: &str) -> (serde_json::Value, serde_json::Value) {
        (
            json!({"id":"example","name":"Example","version":version,"api_version":1,"library":{platform():"plugin.bin"},"drivers":["basic.v1"],"capabilities":[],"contributes":{"pages":[{"id":"home","title":{"en":"Home"}}]}}),
            json!({"format":1,"id":"example","version":version,"api_version":1,"target":target(),"min_host_version":"1.4.8"}),
        )
    }
    fn archive(
        root: &Path,
        manifest: serde_json::Value,
        info: serde_json::Value,
        extra: Vec<(&str, Vec<u8>)>,
    ) -> PathBuf {
        let path = root.join(unique_name("archive")).with_extension("gaplugin");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, bytes) in vec![
            ("plugin.json", serde_json::to_vec(&manifest).unwrap()),
            ("package.json", serde_json::to_vec(&info).unwrap()),
            ("plugin.bin", binary()),
        ]
        .into_iter()
        .chain(extra)
        {
            zip.start_file(
                if name == "plugin.json" && bytes == b"bad" {
                    "plugiN.json"
                } else {
                    name
                },
                SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let duplicate_name = b"plugiN.json";
        for offset in 0..=bytes.len().saturating_sub(duplicate_name.len()) {
            if &bytes[offset..offset + duplicate_name.len()] == duplicate_name {
                bytes[offset..offset + duplicate_name.len()].copy_from_slice(b"plugin.json");
            }
        }
        if bytes
            .windows(duplicate_name.len())
            .any(|name| name == duplicate_name)
        {
            panic!("Duplicate ZIP fixture still contains its temporary filename");
        }
        fs::write(&path, bytes).unwrap();
        path
    }
    fn package(storage: &Storage, version: &str) -> PathBuf {
        let (manifest, info) = package_values(version);
        archive(&storage.root, manifest, info, vec![])
    }
    #[test]
    fn install_update_and_uninstall_are_versioned_and_persist_scope_switches() {
        let storage = storage();
        let first = package(&storage, "0.1.0");
        storage.install(&first).unwrap();
        let old = fs::read(storage.directory("example", "0.1.0").join("plugin.json")).unwrap();
        storage.set_global("example", false).unwrap();
        storage
            .set_repository("D:/Workspace/Test", "example", Some(true))
            .unwrap();
        assert!(
            !storage
                .load()
                .unwrap()
                .enabled("example", "D:/Workspace/Test")
        );
        let second = package(&storage, "0.2.0");
        let pending = storage.install(&second).unwrap();
        assert_eq!(pending.installed["example"].version, "0.1.0");
        assert_eq!(
            pending.installed["example"].pending_version.as_deref(),
            Some("0.2.0")
        );
        assert_eq!(
            fs::read(storage.directory("example", "0.1.0").join("plugin.json")).unwrap(),
            old
        );
        assert_eq!(
            storage.activate_pending().unwrap().installed["example"].version,
            "0.2.0"
        );
        let data = catalog(&storage, &[], false).unwrap();
        let (loaded, errors) = load_enabled(&data);
        assert!(
            loaded.is_empty() && errors.is_empty(),
            "disabled dummy library must never be loaded"
        );
        storage.set_global("example", true).unwrap();
        storage
            .set_repository("D:/Workspace/Test", "example", Some(false))
            .unwrap();
        assert!(
            !storage
                .load()
                .unwrap()
                .enabled("example", "D:/Workspace/Test")
        );
        assert!(
            storage
                .load()
                .unwrap()
                .enabled("example", "D:/Workspace/Other")
        );
        storage.uninstall("example").unwrap();
        assert!(storage.directory("example", "0.2.0").exists());
        let result = storage.activate_pending().unwrap();
        assert!(!result.installed.contains_key("example"));
        assert!(!result.enabled("example", "D:/Workspace/Test"));
        assert!(!storage.root.join("packages/example").exists());
        assert!(
            first.exists() && second.exists(),
            "uninstall must preserve user-supplied archives"
        );
    }
    #[test]
    fn invalid_archive_paths_and_duplicate_entries_leave_no_installed_files() {
        for path in [
            "../escape.txt",
            "/absolute.txt",
            "C:/escape.txt",
            "assets\\escape.txt",
            "assets/../escape.txt",
            "plugin.json",
            "assets/name.",
        ] {
            let storage = storage();
            let (manifest, info) = package_values("0.1.0");
            let zip = archive(&storage.root, manifest, info, vec![(path, b"bad".to_vec())]);
            if path == "plugin.json" {
                let raw = fs::read(&zip).unwrap();
                let footer = raw
                    .windows(4)
                    .rposition(|bytes| bytes == b"PK\x05\x06")
                    .unwrap();
                assert_eq!(
                    u16::from_le_bytes(raw[footer + 10..footer + 12].try_into().unwrap()),
                    4
                );
                let parsed = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
                assert_eq!(
                    parsed.len(),
                    3,
                    "Fixture must contain an exact duplicate, not a case variant"
                );
            }
            assert!(storage.install(&zip).is_err(), "{path}");
            assert!(storage.load().unwrap().installed.is_empty());
            assert!(fs::read_dir(&storage.root).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("staging-")
            }));
        }
    }
    #[test]
    fn incompatible_api_driver_resources_and_target_preserve_installed_version() {
        let storage = storage();
        storage.install(&package(&storage, "0.1.0")).unwrap();
        for mode in ["api", "driver", "dictionary", "target", "host", "version"] {
            let (mut manifest, mut info) = package_values("0.2.0");
            match mode {
                "api" => manifest["api_version"] = json!(99),
                "driver" => manifest["drivers"] = json!(["unknown.v1"]),
                "dictionary" => manifest["locales"] = json!({"en":"../missing.json"}),
                "target" => info["target"] = json!("wrong-architecture"),
                "host" => info["min_host_version"] = json!("999.0.0"),
                _ => info["version"] = json!("0.3.0"),
            }
            let zip = archive(&storage.root, manifest, info, vec![]);
            assert!(storage.install(&zip).is_err(), "{mode}");
            let current = storage.load().unwrap();
            assert_eq!(current.installed["example"].version, "0.1.0");
            assert!(current.installed["example"].pending_version.is_none());
        }
    }
    #[test]
    fn damaged_managed_manifest_keeps_its_management_identity_and_load_error() {
        let storage = storage();
        storage.install(&package(&storage, "0.1.0")).unwrap();
        fs::write(
            storage.directory("example", "0.1.0").join("plugin.json"),
            "broken",
        )
        .unwrap();
        let data = catalog(&storage, &[], false).unwrap();
        assert_eq!(data.entries[0].id, "example");
        assert!(!data.entries[0].error.is_empty());
        storage.uninstall("example").unwrap();
        storage.activate_pending().unwrap();
    }
    #[test]
    fn manager_owns_receiver_before_worker_and_blocks_immediately_until_completion() {
        let storage = storage();
        let mut manager = Manager::idle(storage.clone(), vec![]);
        manager.ready = true;
        let ctx = egui::Context::default();
        manager.start(Operation::Global("example".into(), false), false, &ctx);
        assert!(manager.busy());
        assert!(!manager.enabled("example", "repo"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while manager.busy() {
            manager.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(manager.error.is_empty());
        assert!(!storage.load().unwrap().enabled("example", "repo"));
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        manager.pending = Some(Pending {
            operation: Operation::Refresh,
            receiver,
        });
        manager.poll(&ctx);
        assert!(!manager.busy());
        assert!(manager.error.contains("disconnected"));
    }
    #[test]
    fn inherited_repository_policy_has_no_override_and_normalizes_windows_paths() {
        let storage = storage();
        storage
            .set_repository("D:\\Workspace\\Repo\\", "p", Some(false))
            .unwrap();
        let registry = storage.load().unwrap();
        assert!(!registry.enabled("p", "D:/Workspace/Repo"));
        if cfg!(windows) {
            assert!(!registry.enabled("p", "d:/workspace/repo"));
        }
        storage
            .set_repository("D:/Workspace/Repo", "p", None)
            .unwrap();
        assert!(storage.load().unwrap().repositories.is_empty());
    }
    #[test]
    fn damaged_pending_update_preserves_active_version_and_other_plugins_can_start() {
        let storage = storage();
        storage.install(&package(&storage, "0.1.0")).unwrap();
        storage.install(&package(&storage, "0.2.0")).unwrap();
        fs::write(
            storage.directory("example", "0.2.0").join("plugin.json"),
            "broken",
        )
        .unwrap();
        let registry = storage.activate_pending().unwrap();
        assert_eq!(registry.installed["example"].version, "0.1.0");
        assert!(registry.installed["example"].last_error.is_some());
        let data = catalog(&storage, &[], false).unwrap();
        assert!(data.entries[0].error.is_empty());
    }
    #[test]
    fn managed_manifest_identity_cannot_bypass_a_disabled_registry_entry() {
        let storage = storage();
        storage.install(&package(&storage, "0.1.0")).unwrap();
        storage.set_global("example", false).unwrap();
        let path = storage.directory("example", "0.1.0").join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["id"] = json!("another");
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let data = catalog(&storage, &[], false).unwrap();
        assert_eq!(data.entries[0].id, "example");
        assert!(data.entries[0].error.contains("identity"));
        assert!(load_enabled(&data).0.is_empty());
    }
    #[test]
    fn bundled_install_preserves_switches_newer_versions_and_uninstall_choices() {
        let storage = storage();
        let bundle = storage.root.join("bundled");
        fs::create_dir(&bundle).unwrap();
        let first = package(&storage, "0.1.0");
        fs::copy(&first, bundle.join("example.gaplugin")).unwrap();
        storage.set_global("example", false).unwrap();
        assert!(storage.seed_bundled(&bundle).unwrap().is_empty());
        let registry = storage.load().unwrap();
        assert_eq!(registry.installed["example"].version, "0.1.0");
        assert!(registry.disabled.contains("example"));
        assert!(registry.bundled_seen.contains("example"));
        storage.install(&package(&storage, "0.3.0")).unwrap();
        storage.activate_pending().unwrap();
        assert!(storage.seed_bundled(&bundle).unwrap().is_empty());
        assert_eq!(
            storage.load().unwrap().installed["example"].version,
            "0.3.0"
        );
        storage.uninstall("example").unwrap();
        storage.activate_pending().unwrap();
        storage.seed_bundled(&bundle).unwrap();
        assert!(
            !storage.load().unwrap().installed.contains_key("example"),
            "a removed bundled plugin must not silently return"
        );
    }
}
