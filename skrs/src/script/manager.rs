// src/script/manager.rs

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct ScriptStatus {
    /// normalized name: "test.sk" (never has the leading '-')
    pub name: String,
    pub loaded: bool,
    pub disabled: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ReloadReport {
    pub trigger_count: usize,
    pub errors: Vec<crate::util::diagnostics::SkriptError>,
    pub scripts: Vec<crate::script::script::Script>,
}

pub struct ScriptManager {
    /// Where scripts live on disk
    scripts_dir: PathBuf,
    /// Key = normalized script name ("test.sk")
    scripts: HashMap<String, ScriptStatus>,
}

impl ScriptManager {
    /// Use the same dir as your loader.
    pub fn new() -> Self {
        Self::with_dir(Path::new("plugins/skrs/scripts"))
    }

    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            scripts_dir: dir.into(),
            scripts: HashMap::new(),
        }
    }

    pub fn scripts_dir(&self) -> &Path {
        &self.scripts_dir
    }

    pub fn list(&self) -> Vec<ScriptStatus> {
        self.scripts.values().cloned().collect()
    }

    pub fn info(&self, name: &str) -> Option<ScriptStatus> {
        self.scripts.get(&normalize_name(name)).cloned()
    }

    // -----------------------------
    // helpers for tab-complete
    // -----------------------------

    /// All known scripts (enabled + disabled), normalized names like "test.sk".
    pub fn all_script_names(&self) -> Vec<String> {
        self.scripts.keys().cloned().collect()
    }

    /// Scripts that are currently enabled on disk (NOT prefixed with '-').
    pub fn enabled_script_names(&self) -> Vec<String> {
        self.scripts
            .values()
            .filter(|s| !s.disabled)
            .map(|s| s.name.clone())
            .collect()
    }

    /// Scripts that are currently disabled on disk (prefixed with '-').
    pub fn disabled_script_names(&self) -> Vec<String> {
        self.scripts
            .values()
            .filter(|s| s.disabled)
            .map(|s| s.name.clone())
            .collect()
    }

    fn enabled_path(&self, name: &str) -> PathBuf {
        let n = normalize_name(name);
        self.scripts_dir.join(n)
    }

    fn disabled_path(&self, name: &str) -> PathBuf {
        let n = normalize_name(name);
        self.scripts_dir.join(format!("-{n}"))
    }

    /// Ensure the scripts map contains every *.sk file on disk, including disabled ones (-*.sk).
    /// Returns how many *.sk files were found (enabled + disabled).
    pub fn sync_from_disk(&mut self) -> Result<usize, String> {
        let dir = &self.scripts_dir;

        // If the folder doesn't exist yet, treat it as empty (don’t hard-fail).
        if !dir.exists() {
            self.scripts.clear();
            return Ok(0);
        }

        let rd = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read scripts dir {:?}: {e}", dir))?;

        let mut found_keys: HashSet<String> = HashSet::new();
        let mut found_count = 0usize;

        for entry in rd.flatten() {
            let path = entry.path();

            let is_sk = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("sk"))
                .unwrap_or(false);

            if !is_sk {
                continue;
            }

            let file = match path.file_name().and_then(|s| s.to_str()) {
                Some(f) => f,
                None => continue,
            };

            let is_disabled = file.starts_with('-');
            let display_name = if is_disabled { &file[1..] } else { file };

            let key = normalize_name(display_name);
            found_keys.insert(key.clone());
            found_count += 1;

            self.scripts
                .entry(key.clone())
                .and_modify(|s| {
                    s.disabled = is_disabled;
                    if is_disabled {
                        s.loaded = false;
                        s.error = None;
                    }
                })
                .or_insert(ScriptStatus {
                    name: key,
                    loaded: false,
                    disabled: is_disabled,
                    error: None,
                });
        }

        // Prune entries no longer on disk (enabled or disabled)
        self.scripts.retain(|k, _| found_keys.contains(k));

        Ok(found_count)
    }

    /// Mark a script loaded (and clear error). Call this from your loader after a successful load.
    pub fn mark_loaded(&mut self, name: &str) {
        let key = normalize_name(name);
        self.scripts
            .entry(key.clone())
            .and_modify(|s| {
                s.loaded = true;
                s.disabled = false;
                s.error = None;
            })
            .or_insert(ScriptStatus {
                name: key,
                loaded: true,
                disabled: false,
                error: None,
            });
    }

    /// Mark a script errored. Call this from your loader if parse/load fails.
    pub fn mark_error(&mut self, name: &str, err: String) {
        let key = normalize_name(name);
        self.scripts
            .entry(key.clone())
            .and_modify(|s| {
                s.loaded = false;
                // keep disabled as-is; errors can happen on enabled scripts
                s.error = Some(err.clone());
            })
            .or_insert(ScriptStatus {
                name: key,
                loaded: false,
                disabled: false,
                error: Some(err),
            });
    }

    /// Disable a script by renaming `name.sk` -> `-name.sk`.
    pub fn disable(&mut self, name: &str) -> Result<(), String> {
        let key = normalize_name(name);

        let from = self.enabled_path(&key);
        let to = self.disabled_path(&key);

        if !from.exists() {
            if to.exists() {
                // already disabled
                self.scripts
                    .entry(key.clone())
                    .and_modify(|s| {
                        s.disabled = true;
                        s.loaded = false;
                        s.error = None;
                    })
                    .or_insert(ScriptStatus {
                        name: key,
                        loaded: false,
                        disabled: true,
                        error: None,
                    });
                return Ok(());
            }

            return Err(format!("Script not found: {key}"));
        }

        // unload first (best effort)
        let _ = self.unload(&key);

        fs::rename(&from, &to).map_err(|e| format!("Failed to disable {key}: {e}"))?;

        self.scripts
            .entry(key.clone())
            .and_modify(|s| {
                s.disabled = true;
                s.loaded = false;
                s.error = None;
            })
            .or_insert(ScriptStatus {
                name: key,
                loaded: false,
                disabled: true,
                error: None,
            });

        Ok(())
    }

    /// Enable a script by renaming `-name.sk` -> `name.sk`.
    pub fn enable(&mut self, name: &str) -> Result<(), String> {
        let key = normalize_name(name);

        let from = self.disabled_path(&key);
        let to = self.enabled_path(&key);

        if !from.exists() {
            if to.exists() {
                // already enabled
                self.scripts
                    .entry(key.clone())
                    .and_modify(|s| {
                        s.disabled = false;
                    })
                    .or_insert(ScriptStatus {
                        name: key,
                        loaded: false,
                        disabled: false,
                        error: None,
                    });
                return Ok(());
            }

            return Err(format!("Script not found: {key}"));
        }

        fs::rename(&from, &to).map_err(|e| format!("Failed to enable {key}: {e}"))?;

        self.scripts
            .entry(key.clone())
            .and_modify(|s| {
                s.disabled = false;
                s.error = None;
            })
            .or_insert(ScriptStatus {
                name: key,
                loaded: false,
                disabled: false,
                error: None,
            });

        Ok(())
    }

    /// Load one script by name.
    ///
    /// NOTE: For now this only updates bookkeeping. Real loading is done by reload_all()
    /// which reparses scripts from disk and replaces the runtime trigger registry.
    pub fn load(&mut self, name: &str) -> Result<(), String> {
        let key = normalize_name(name);

        let _ = self.sync_from_disk();

        if let Some(st) = self.scripts.get(&key) {
            if st.disabled {
                return Err(format!("Script '{key}' is disabled (rename -{key} back to enable)."));
            }
        }

        self.scripts.entry(key.clone()).or_insert(ScriptStatus {
            name: key.clone(),
            loaded: false,
            disabled: false,
            error: None,
        });

        self.mark_loaded(&key);
        Ok(())
    }

    /// Unload one script by name.
    ///
    /// NOTE: Per-script unregistering is not implemented yet; runtime is managed by reload_all().
    pub fn unload(&mut self, name: &str) -> Result<(), String> {
        let key = normalize_name(name);

        match self.scripts.get_mut(&key) {
            Some(s) => {
                s.loaded = false;
                s.error = None;
                Ok(())
            }
            None => Err(format!("Unknown script '{key}'")),
        }
    }

    pub fn reload(&mut self, name: &str) -> Result<(), String> {
        let key = normalize_name(name);

        if let Some(st) = self.scripts.get(&key) {
            if st.disabled {
                return Err(format!("Script '{key}' is disabled."));
            }
        }

        let _ = self.unload(&key);
        self.load(&key)
    }

    /// FULL reload:
    /// - Re-scan disk for enabled/disabled scripts
    /// - Load & parse enabled scripts via loader.rs (disabled -*.sk are ignored there)
    /// - Replace the global trigger registry used by runtime::dispatcher (get_triggers)
    pub fn reload_all(&mut self) -> Result<ReloadReport, String> {
        use std::path::Path;

        // Refresh status from disk
        self.sync_from_disk()?;

        // Reset loaded/error state for everything (enabled scripts will be marked loaded below)
        for st in self.scripts.values_mut() {
            st.loaded = false;
            st.error = None;
        }

        // Load enabled scripts from disk (loader skips "-*.sk")
        let loaded = crate::script::loader::load_scripts();
        let scripts = loaded.scripts.clone();

        // Replace the runtime trigger registry that get_triggers() reads from
        let trigger_count =
            crate::registry::events::replace_all_triggers_from_scripts(loaded.scripts.clone());

        // Mark loaded scripts as loaded
        for sc in &loaded.scripts {
            if let super::script::ScriptSource::File { path } = &sc.source {
                if let Some(name) = Path::new(path).file_name().and_then(|s| s.to_str()) {
                    self.mark_loaded(name);
                }
            }
        }

        // Mark scripts with errors as not loaded + attach a short error summary
        for err in &loaded.errors {
            if let Some(name) = Path::new(&err.file).file_name().and_then(|s| s.to_str()) {
                self.mark_error(name, format!("{} (line {})", err.message, err.line));
            }
        }

        Ok(ReloadReport {
            trigger_count,
            errors: loaded.errors,
            scripts,
        })
    }
}

// module-scope helper (so methods can call normalize_name(...))
fn normalize_name(name: &str) -> String {
    // accept user typing "-test.sk" or "test" etc
    let mut s = name.trim().to_string();
    if let Some(rest) = s.strip_prefix('-') {
        s = rest.to_string();
    }

    if !s.to_ascii_lowercase().ends_with(".sk") {
        s.push_str(".sk");
    }

    s.to_ascii_lowercase()
}

// module-scope alias (so other modules can import it)
pub type SharedScriptManager = Arc<RwLock<ScriptManager>>;
