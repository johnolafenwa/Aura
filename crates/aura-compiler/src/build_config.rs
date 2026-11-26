//! The configuration shared by all modules of one compilation.
use crate::{package, Diagnostic, FreestandingTarget, HostedTarget, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildConfig {
    pub target: String,
    pub os: String,
    pub arch: String,
    pub pointer_width: u32,
    pub endianness: String,
    pub freestanding: bool,
    pub hosted: bool,
    pub debug: bool,
    pub release: bool,
    pub cfg: BTreeMap<String, String>,
}

const NATIVE_ONLY: [(&str, &str, &str); 3] = [
    ("x86_64-apple-darwin", "macos", "x86_64"),
    ("aarch64-apple-darwin", "macos", "aarch64"),
    ("x86_64-pc-windows-msvc", "windows", "x86_64"),
];

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            target: env!("AURA_COMPILER_TARGET").into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            pointer_width: usize::BITS,
            endianness: if cfg!(target_endian = "big") {
                "big"
            } else {
                "little"
            }
            .into(),
            freestanding: false,
            hosted: true,
            debug: false,
            release: true,
            cfg: BTreeMap::new(),
        }
    }
}

impl BuildConfig {
    /// The named target's properties, including when checking for a different host.
    pub fn for_target(target: &str, debug: bool) -> Option<Self> {
        let (os, arch) = if let Some(hosted) = HostedTarget::parse(target) {
            ("linux", hosted.arch())
        } else if let Some(bare) = FreestandingTarget::parse(target) {
            ("none", bare.arch())
        } else {
            let (_, os, arch) = NATIVE_ONLY
                .iter()
                .find(|(triple, _, _)| *triple == target)?;
            (*os, *arch)
        };
        // Every triple in the supported target inventory has a 64-bit, little-endian ABI.
        Some(Self {
            target: target.into(),
            os: os.into(),
            arch: arch.into(),
            pointer_width: 64,
            endianness: "little".into(),
            freestanding: os == "none",
            hosted: os != "none",
            debug,
            release: !debug,
            cfg: BTreeMap::new(),
        })
    }

    /// Check the same feature selection and profile on each supported target.
    pub fn supported_targets(&self) -> Vec<(String, Self)> {
        HostedTarget::ALL
            .into_iter()
            .map(|t| t.name())
            .chain(NATIVE_ONLY.iter().map(|(triple, _, _)| *triple))
            .chain(FreestandingTarget::ALL.into_iter().map(|t| t.name()))
            .map(|name| {
                let mut config = Self::for_target(name, self.debug).expect("supported target");
                config.cfg.clone_from(&self.cfg);
                (name.into(), config)
            })
            .collect()
    }

    /// Combine the entry package's defaults with any explicit options in scope.
    pub fn for_path(path: &Path) -> Result<Self> {
        let manifest = package::manifest_cfg_for_entry(path)?;
        Ok(current_build_config().with_manifest(&manifest))
    }

    pub(crate) fn with_manifest(mut self, manifest: &BTreeMap<String, String>) -> Self {
        for (name, value) in manifest {
            self.cfg
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        self
    }

    /// Set a flag or a named value, preserving any later equals signs in the value.
    pub fn apply_option(&mut self, option: &str) -> Result<()> {
        let (name, value) = option.split_once('=').unwrap_or((option, "true"));
        validate_name(name)?;
        self.cfg.insert(name.into(), value.into());
        Ok(())
    }

    /// A canonical, unambiguous input to the native artifact fingerprint.
    pub fn cache_key_material(&self) -> Vec<u8> {
        let mut out = (self.cfg.len() as u64).to_le_bytes().to_vec();
        let mut field = |s: &str| {
            out.extend_from_slice(&(s.len() as u64).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        };
        for value in [&self.target, &self.os, &self.arch, &self.endianness] {
            field(value);
        }
        for (name, value) in &self.cfg {
            field(name);
            field(value);
        }
        out.extend_from_slice(&self.pointer_width.to_le_bytes());
        out.extend([self.freestanding, self.hosted, self.debug, self.release].map(u8::from));
        out
    }

    /// Install this configuration for the lifetime of the returned guard.
    #[must_use]
    pub fn enter(self) -> BuildConfigGuard {
        let saved_namespaces = CONFIG.with_borrow(|previous| {
            (previous != &self).then(|| {
                let mut previous = BTreeMap::new();
                crate::builtin_sources::swap_checked_sources(&mut previous);
                previous
            })
        });
        BuildConfigGuard {
            saved_namespaces,
            previous: CONFIG.replace(self),
            thread: PhantomData,
        }
    }
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    if !package::is_valid_package_name(name) {
        return Err(Diagnostic::coded(
            "AU2070",
            format!("invalid build cfg name `{name}`: expected an Aura identifier"),
        ));
    }
    Ok(())
}

thread_local! {
    static CONFIG: RefCell<BuildConfig> = RefCell::new(BuildConfig::default());
}

pub struct BuildConfigGuard {
    saved_namespaces: Option<BTreeMap<String, crate::sema::ModuleNamespace>>,
    previous: BuildConfig,
    // Restoring configuration must happen on the thread that installed it.
    thread: PhantomData<Rc<()>>,
}

impl Drop for BuildConfigGuard {
    fn drop(&mut self) {
        if let Some(cache) = self.saved_namespaces.as_mut() {
            crate::builtin_sources::swap_checked_sources(cache);
        }
        CONFIG.with_borrow_mut(|cfg| std::mem::swap(cfg, &mut self.previous));
    }
}

pub fn current_build_config() -> BuildConfig {
    CONFIG.with_borrow(Clone::clone)
}

pub fn with_build_config<R>(config: BuildConfig, work: impl FnOnce() -> R) -> R {
    let _scope = config.enter();
    work()
}
