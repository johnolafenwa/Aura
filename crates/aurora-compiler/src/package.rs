use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{
    ffi::CString,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::ffi::OsStrExt,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::diag::{Diagnostic, Result};

const MANIFEST_NAME: &str = "Aurora.toml";
const LOCKFILE_NAME: &str = "Aurora.lock";
const LOCKFILE_VERSION: u32 = 1;
const SUPPORTED_PACKAGE_EDITION: &str = "2026";
const MAX_DEPENDENCIES_PER_PACKAGE: usize = 1024;
const MAX_PACKAGES_IN_GRAPH: usize = 4096;
const TEMP_FILE_RETRY_LIMIT: usize = 32;
const DEFAULT_GIT_COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(60);

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
enum PackageOrigin {
    Path,
    Git {
        source: String,
        rev: String,
        selector: GitSelector,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum GitSelector {
    Rev(String),
    Tag(String),
    Branch(String),
}

impl GitSelector {
    fn from_manifest(
        dependency_name: &str,
        rev: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
    ) -> Result<Self> {
        let selector_count =
            usize::from(rev.is_some()) + usize::from(tag.is_some()) + usize::from(branch.is_some());
        if selector_count > 1 {
            return Err(Diagnostic::new(format!(
                "dependency `{}` must choose at most one git selector: `rev`, `tag`, or `branch`",
                dependency_name
            )));
        }
        if let Some(rev) = rev {
            if rev.trim().is_empty() {
                return Err(Diagnostic::new(format!(
                    "dependency `{}` has an empty git revision",
                    dependency_name
                )));
            }
            validate_git_revision_literal(dependency_name, &rev)?;
            return Ok(Self::Rev(rev));
        }
        if let Some(tag) = tag {
            validate_git_selector_literal(dependency_name, "tag", &tag)?;
            if tag.trim().is_empty() {
                return Err(Diagnostic::new(format!(
                    "dependency `{}` has an empty git tag",
                    dependency_name
                )));
            }
            return Ok(Self::Tag(tag));
        }
        let branch = branch.unwrap_or_else(|| "main".to_string());
        validate_git_selector_literal(dependency_name, "branch", &branch)?;
        if branch.trim().is_empty() {
            return Err(Diagnostic::new(format!(
                "dependency `{}` has an empty git branch",
                dependency_name
            )));
        }
        Ok(Self::Branch(branch))
    }

    fn from_lockfile(
        package_name: &str,
        rev: &str,
        tag: Option<String>,
        branch: Option<String>,
    ) -> Result<Self> {
        let selector_count = usize::from(tag.is_some()) + usize::from(branch.is_some());
        if selector_count > 1 {
            return Err(Diagnostic::new(format!(
                "lockfile entry for package `{}` contains multiple git selectors",
                package_name
            )));
        }
        if let Some(tag) = tag {
            validate_git_selector_literal(package_name, "tag", &tag)?;
            return Ok(Self::Tag(tag));
        }
        if let Some(branch) = branch {
            validate_git_selector_literal(package_name, "branch", &branch)?;
            return Ok(Self::Branch(branch));
        }
        validate_git_revision_literal(package_name, rev)?;
        Ok(Self::Rev(rev.to_string()))
    }

    fn write_lockfile_fields(&self, output: &mut String) {
        match self {
            Self::Rev(_) => {}
            Self::Tag(tag) => output.push_str(&format!("tag = {}\n", toml_string(tag))),
            Self::Branch(branch) => output.push_str(&format!("branch = {}\n", toml_string(branch))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PackageSource {
    pub name: String,
    pub version: String,
    pub manifest_dir: PathBuf,
    pub source_root: PathBuf,
    canonical_source_root: PathBuf,
    pub external_prefix: Option<String>,
    pub dependencies: BTreeMap<String, String>,
    origin: PackageOrigin,
}

#[derive(Clone, Debug)]
pub struct PackageGraph {
    pub root_source_root: PathBuf,
    pub lockfile_root: PathBuf,
    packages: BTreeMap<String, PackageSource>,
}

impl PackageGraph {
    pub fn discover_for_entry(entry_path: &Path) -> Result<Option<Self>> {
        let normalized_entry = canonicalize_if_exists(entry_path)?;
        let Some(root_manifest_dir) = find_enclosing_package_manifest_dir(&normalized_entry)?
        else {
            return Ok(None);
        };
        let workspace_root = find_workspace_root(&root_manifest_dir)?;
        let lockfile_root = workspace_root
            .clone()
            .unwrap_or_else(|| root_manifest_dir.clone());
        let locked_packages = load_lockfile(&lockfile_root)?;

        let mut resolver = PackageResolver::new(locked_packages, DependencyRefreshPolicy::None);
        let root_package_name =
            resolver.resolve_package(&root_manifest_dir, None, PackageOrigin::Path)?;
        let mut packages = resolver.packages;
        for (name, package) in packages.iter_mut() {
            if name != &root_package_name {
                package.external_prefix = Some(name.clone());
            }
        }
        let root_source_root = packages
            .get(&root_package_name)
            .ok_or_else(|| Diagnostic::new("internal error: root package should be resolved"))?
            .canonical_source_root
            .clone();

        if !normalized_entry.starts_with(&root_source_root) {
            return Err(Diagnostic::new(format!(
                "entry `{}` is outside package source root `{}`",
                normalized_entry.display(),
                root_source_root.display()
            )));
        }

        Ok(Some(Self {
            root_source_root,
            lockfile_root,
            packages,
        }))
    }

    pub fn source_for_path(&self, path: &Path) -> Option<&PackageSource> {
        self.packages
            .values()
            .filter(|package| path.starts_with(&package.canonical_source_root))
            .max_by_key(|package| package.canonical_source_root.as_os_str().len())
    }

    pub fn module_name_for_path(&self, path: &Path) -> Option<String> {
        let package = self.source_for_path(path)?;
        let relative = path.strip_prefix(&package.canonical_source_root).ok()?;
        let mut without_extension = relative.to_path_buf();
        without_extension.set_extension("");
        let logical = without_extension
            .iter()
            .map(|segment| segment.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(".");
        Some(match &package.external_prefix {
            Some(prefix) if !logical.is_empty() => format!("{}.{}", prefix, logical),
            Some(prefix) => prefix.clone(),
            None => logical,
        })
    }

    pub fn dependency_aliases_for_path(&self, path: &Path) -> BTreeSet<String> {
        self.source_for_path(path)
            .map(|package| package.dependencies.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn resolve_import_path(
        &self,
        current_path: &Path,
        module_path: &[String],
    ) -> Result<PathBuf> {
        let current_package = self.source_for_path(current_path).ok_or_else(|| {
            Diagnostic::new(format!(
                "could not determine package source root for `{}`",
                current_path.display()
            ))
        })?;

        let (target_package, relative_segments) = if let Some(first) = module_path.first() {
            if let Some(package_name) = current_package.dependencies.get(first) {
                let package = self.packages.get(package_name).ok_or_else(|| {
                    Diagnostic::new(format!(
                        "resolved dependency `{}` is missing from the package graph",
                        package_name
                    ))
                })?;
                (package, &module_path[1..])
            } else {
                (current_package, module_path)
            }
        } else {
            (current_package, module_path)
        };

        checked_source_file_path(
            &target_package.source_root,
            &target_package.canonical_source_root,
            relative_segments,
        )
    }

    pub fn write_lockfile(&self) -> Result<()> {
        let lockfile_path = self.lockfile_root.join(LOCKFILE_NAME);
        let mut packages = self.packages.values().collect::<Vec<_>>();
        packages.sort_by(|left, right| left.name.cmp(&right.name));

        let mut source = format!("version = {}\n", LOCKFILE_VERSION);
        for package in packages {
            source.push_str("\n[[package]]\n");
            source.push_str(&format!("name = {}\n", toml_string(&package.name)));
            source.push_str(&format!("version = {}\n", toml_string(&package.version)));
            match &package.origin {
                PackageOrigin::Path => {
                    let relative_path =
                        relative_path_from(&self.lockfile_root, &package.manifest_dir);
                    source.push_str("source = \"path\"\n");
                    source.push_str(&format!(
                        "path = {}\n",
                        toml_string(&relative_path.display().to_string())
                    ));
                }
                PackageOrigin::Git {
                    source: git_source,
                    rev,
                    selector,
                } => {
                    source.push_str("source = \"git\"\n");
                    source.push_str(&format!("git = {}\n", toml_string(git_source)));
                    source.push_str(&format!("rev = {}\n", toml_string(rev)));
                    selector.write_lockfile_fields(&mut source);
                }
            }
        }

        write_atomic_file(
            &lockfile_path,
            source.as_bytes(),
            "lockfile",
            &format!("`{}`", lockfile_path.display()),
        )
    }
}

pub fn update_git_dependencies_in_working_dir(
    start_dir: &Path,
    target_package: Option<&str>,
) -> Result<DependencyUpdateResult> {
    let start_dir = canonicalize_dir(start_dir)?;
    let refresh_policy = match target_package {
        Some(package) => DependencyRefreshPolicy::Selected(package.to_string()),
        None => DependencyRefreshPolicy::AllGit,
    };

    let (graph, updated_packages) =
        resolve_package_graph_for_update(&start_dir, refresh_policy.clone())?;

    if let Some(package) = target_package {
        let Some(resolved) = graph.packages.get(package) else {
            return Err(Diagnostic::new(format!(
                "package `{}` is not part of the current package graph",
                package
            )));
        };
        if !matches!(resolved.origin, PackageOrigin::Git { .. }) {
            return Err(Diagnostic::new(format!(
                "package `{}` is not a git dependency in the current package graph",
                package
            )));
        }
    }

    graph.write_lockfile()?;
    Ok(DependencyUpdateResult {
        updated_packages,
        lockfile_root: graph.lockfile_root,
    })
}

#[derive(Clone, Debug)]
enum LockedPackageSource {
    Path,
    Git {
        source: String,
        rev: String,
        selector: GitSelector,
    },
}

#[derive(Clone, Debug)]
struct LockedPackage {
    source: LockedPackageSource,
}

impl LockedPackage {
    fn git_locked_rev(&self, source: &str, selector: &GitSelector) -> Option<String> {
        match &self.source {
            LockedPackageSource::Git {
                source: locked_source,
                rev,
                selector: locked_selector,
            } if locked_source == source && locked_selector == selector => Some(rev.clone()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
enum DependencyRefreshPolicy {
    #[default]
    None,
    AllGit,
    Selected(String),
}

impl DependencyRefreshPolicy {
    fn should_refresh(&self, dependency_name: &str, selector: &GitSelector) -> bool {
        if matches!(selector, GitSelector::Rev(_)) {
            return false;
        }
        match self {
            Self::None => false,
            Self::AllGit => true,
            Self::Selected(target) => target == dependency_name,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DependencyUpdateResult {
    pub updated_packages: Vec<String>,
    pub lockfile_root: PathBuf,
}

struct PackageResolver {
    packages: BTreeMap<String, PackageSource>,
    in_progress: Vec<PathBuf>,
    locked_packages: BTreeMap<String, LockedPackage>,
    refresh_policy: DependencyRefreshPolicy,
    refreshed_packages: BTreeSet<String>,
}

impl PackageResolver {
    fn new(
        locked_packages: BTreeMap<String, LockedPackage>,
        refresh_policy: DependencyRefreshPolicy,
    ) -> Self {
        Self {
            packages: BTreeMap::new(),
            in_progress: Vec::new(),
            locked_packages,
            refresh_policy,
            refreshed_packages: BTreeSet::new(),
        }
    }

    fn resolve_package(
        &mut self,
        manifest_dir: &Path,
        expected_name: Option<&str>,
        origin: PackageOrigin,
    ) -> Result<String> {
        let manifest_dir = canonicalize_dir(manifest_dir)?;
        if self.in_progress.contains(&manifest_dir) {
            return Err(Diagnostic::new(format!(
                "cyclic package dependency involving `{}`",
                manifest_dir.display()
            )));
        }

        let manifest = load_package_manifest(&manifest_dir)?;
        if let Some(expected_name) = expected_name {
            if manifest.package.name != expected_name {
                return Err(Diagnostic::new(format!(
                    "dependency name `{}` does not match package name `{}` at `{}`",
                    expected_name,
                    manifest.package.name,
                    manifest_dir.display()
                )));
            }
        }

        if let Some(existing) = self.packages.get(&manifest.package.name) {
            if existing.manifest_dir != manifest_dir {
                return Err(Diagnostic::new(format!(
                    "package name `{}` resolves to multiple paths: `{}` and `{}`",
                    manifest.package.name,
                    existing.manifest_dir.display(),
                    manifest_dir.display()
                )));
            }
            return Ok(existing.name.clone());
        }

        if self.packages.len() >= MAX_PACKAGES_IN_GRAPH {
            return Err(Diagnostic::new(format!(
                "package graph exceeds the supported limit of {} packages while resolving `{}`",
                MAX_PACKAGES_IN_GRAPH, manifest.package.name
            )));
        }

        if manifest.dependencies.len() > MAX_DEPENDENCIES_PER_PACKAGE {
            return Err(Diagnostic::new(format!(
                "package `{}` declares {} dependencies, which exceeds the supported limit of {}",
                manifest.package.name,
                manifest.dependencies.len(),
                MAX_DEPENDENCIES_PER_PACKAGE
            )));
        }

        self.in_progress.push(manifest_dir.clone());
        let mut dependencies = BTreeMap::new();
        for (name, dependency) in manifest.dependencies {
            let dependency_name = match dependency {
                RawDependency::Version(version) => {
                    return Err(unsupported_version_dependency(&name, Some(&version)));
                }
                RawDependency::Detailed {
                    path,
                    version,
                    git,
                    rev,
                    tag,
                    branch,
                } => {
                    validate_dependency_shape(
                        &name,
                        path.as_deref(),
                        version.as_deref(),
                        git.as_deref(),
                        rev.as_deref(),
                        tag.as_deref(),
                        branch.as_deref(),
                    )?;

                    if let Some(path) = path {
                        let dependency_dir = canonicalize_dir(&manifest_dir.join(path))?;
                        self.resolve_package(&dependency_dir, Some(&name), PackageOrigin::Path)?
                    } else {
                        let selector = GitSelector::from_manifest(&name, rev, tag, branch)?;
                        let should_refresh = self.refresh_policy.should_refresh(&name, &selector);
                        let locked = if should_refresh {
                            None
                        } else {
                            self.locked_packages.get(&name)
                        };
                        let git_source = git.ok_or_else(|| {
                            Diagnostic::new(format!(
                                "dependency `{}` must specify `git = ...` for git resolution",
                                name
                            ))
                        })?;
                        let resolved = resolve_git_dependency(
                            &manifest_dir,
                            &name,
                            git_source,
                            &selector,
                            locked,
                        )?;
                        if should_refresh {
                            let previous_rev =
                                self.locked_packages.get(&name).and_then(|package| {
                                    package.git_locked_rev(&resolved.normalized_source, &selector)
                                });
                            if previous_rev.as_deref() != Some(resolved.resolved_rev.as_str()) {
                                self.refreshed_packages.insert(name.clone());
                            }
                        }
                        self.resolve_package(
                            &resolved.checkout_dir,
                            Some(&name),
                            PackageOrigin::Git {
                                source: resolved.normalized_source,
                                rev: resolved.resolved_rev,
                                selector: resolved.selector,
                            },
                        )?
                    }
                }
            };
            dependencies.insert(name, dependency_name);
        }

        let source_root = manifest_dir.join("src");
        let canonical_source_root = canonicalize_dir(&source_root)?;
        let package = PackageSource {
            name: manifest.package.name.clone(),
            version: manifest.package.version,
            manifest_dir: manifest_dir.clone(),
            source_root,
            canonical_source_root,
            external_prefix: None,
            dependencies,
            origin,
        };
        self.packages.insert(package.name.clone(), package);
        self.in_progress.pop();
        Ok(manifest.package.name)
    }
}

#[derive(Clone, Debug)]
struct ResolvedGitDependency {
    checkout_dir: PathBuf,
    normalized_source: String,
    resolved_rev: String,
    selector: GitSelector,
}

#[derive(Deserialize)]
struct RawManifest {
    package: Option<RawPackageSection>,
    #[serde(default)]
    dependencies: BTreeMap<String, RawDependency>,
    workspace: Option<RawWorkspaceSection>,
}

#[derive(Deserialize)]
struct RawPackageSection {
    name: String,
    version: String,
    edition: String,
}

#[derive(Deserialize)]
struct RawWorkspaceSection {
    #[serde(default)]
    members: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDependency {
    Version(String),
    Detailed {
        path: Option<String>,
        version: Option<String>,
        git: Option<String>,
        rev: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
    },
}

#[derive(Deserialize)]
struct RawLockfile {
    version: Option<u32>,
    #[serde(default)]
    package: Vec<RawLockPackage>,
}

#[derive(Deserialize)]
struct RawLockPackage {
    name: String,
    #[serde(rename = "version")]
    _version: String,
    source: String,
    path: Option<String>,
    git: Option<String>,
    rev: Option<String>,
    tag: Option<String>,
    branch: Option<String>,
}

struct ParsedPackageManifest {
    package: RawPackageSection,
    dependencies: BTreeMap<String, RawDependency>,
}

fn find_enclosing_package_manifest_dir(entry_path: &Path) -> Result<Option<PathBuf>> {
    let entry_dir = entry_path.parent().unwrap_or_else(|| Path::new("."));
    for ancestor in entry_dir.ancestors() {
        let manifest_path = ancestor.join(MANIFEST_NAME);
        if !manifest_path.exists() {
            continue;
        }
        let raw = load_raw_manifest(&manifest_path)?;
        if raw.package.is_some() {
            return Ok(Some(ancestor.to_path_buf()));
        }
    }
    Ok(None)
}

fn find_workspace_root(package_manifest_dir: &Path) -> Result<Option<PathBuf>> {
    for ancestor in package_manifest_dir.ancestors() {
        let manifest_path = ancestor.join(MANIFEST_NAME);
        if !manifest_path.exists() {
            continue;
        }
        let raw = load_raw_manifest(&manifest_path)?;
        let Some(workspace) = raw.workspace else {
            continue;
        };
        let Ok(relative_member_path) = package_manifest_dir.strip_prefix(ancestor) else {
            continue;
        };
        let relative_member_path = normalize_relative_path(relative_member_path);
        if workspace
            .members
            .iter()
            .any(|member| normalize_member_path(member) == relative_member_path)
        {
            return Ok(Some(ancestor.to_path_buf()));
        }
    }
    Ok(None)
}

fn find_enclosing_package_manifest_dir_from_dir(start_dir: &Path) -> Result<Option<PathBuf>> {
    for ancestor in start_dir.ancestors() {
        let manifest_path = ancestor.join(MANIFEST_NAME);
        if !manifest_path.exists() {
            continue;
        }
        let raw = load_raw_manifest(&manifest_path)?;
        if raw.package.is_some() {
            return Ok(Some(ancestor.to_path_buf()));
        }
    }
    Ok(None)
}

fn find_enclosing_workspace_root_from_dir(start_dir: &Path) -> Result<Option<PathBuf>> {
    for ancestor in start_dir.ancestors() {
        let manifest_path = ancestor.join(MANIFEST_NAME);
        if !manifest_path.exists() {
            continue;
        }
        let raw = load_raw_manifest(&manifest_path)?;
        if raw.workspace.is_some() {
            return Ok(Some(ancestor.to_path_buf()));
        }
    }
    Ok(None)
}

fn resolve_package_graph_for_update(
    start_dir: &Path,
    refresh_policy: DependencyRefreshPolicy,
) -> Result<(PackageGraph, Vec<String>)> {
    if let Some(package_manifest_dir) = find_enclosing_package_manifest_dir_from_dir(start_dir)? {
        let workspace_root = find_workspace_root(&package_manifest_dir)?;
        let lockfile_root = workspace_root
            .clone()
            .unwrap_or_else(|| package_manifest_dir.clone());
        let locked_packages = load_lockfile(&lockfile_root)?;
        let mut resolver = PackageResolver::new(locked_packages, refresh_policy);
        let root_package_name =
            resolver.resolve_package(&package_manifest_dir, None, PackageOrigin::Path)?;
        let mut packages = resolver.packages;
        for (name, package) in packages.iter_mut() {
            if name != &root_package_name {
                package.external_prefix = Some(name.clone());
            }
        }
        let root_source_root = packages
            .get(&root_package_name)
            .ok_or_else(|| Diagnostic::new("internal error: resolved root package should exist"))?
            .canonical_source_root
            .clone();
        let updated_packages = resolver.refreshed_packages.into_iter().collect::<Vec<_>>();
        return Ok((
            PackageGraph {
                root_source_root,
                lockfile_root,
                packages,
            },
            updated_packages,
        ));
    }

    let Some(workspace_root) = find_enclosing_workspace_root_from_dir(start_dir)? else {
        return Err(Diagnostic::new(format!(
            "could not find an enclosing Aurora package or workspace starting from `{}`",
            start_dir.display()
        )));
    };

    let locked_packages = load_lockfile(&workspace_root)?;
    let mut resolver = PackageResolver::new(locked_packages, refresh_policy);
    let member_dirs = load_workspace_member_dirs(&workspace_root)?;
    if member_dirs.is_empty() {
        return Err(Diagnostic::new(format!(
            "workspace `{}` does not declare any members",
            workspace_root.display()
        )));
    }

    let mut root_source_root = workspace_root.clone();
    let mut root_packages = BTreeSet::new();
    for member_dir in member_dirs {
        let package_name = resolver.resolve_package(&member_dir, None, PackageOrigin::Path)?;
        root_packages.insert(package_name.clone());
        if root_source_root == workspace_root {
            root_source_root = resolver
                .packages
                .get(&package_name)
                .ok_or_else(|| {
                    Diagnostic::new("internal error: resolved workspace member should exist")
                })?
                .canonical_source_root
                .clone();
        }
    }

    let mut packages = resolver.packages;
    for (name, package) in packages.iter_mut() {
        if !root_packages.contains(name) {
            package.external_prefix = Some(name.clone());
        }
    }

    let updated_packages = resolver.refreshed_packages.into_iter().collect::<Vec<_>>();
    Ok((
        PackageGraph {
            root_source_root,
            lockfile_root: workspace_root,
            packages,
        },
        updated_packages,
    ))
}

fn load_workspace_member_dirs(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let manifest_path = workspace_root.join(MANIFEST_NAME);
    let raw = load_raw_manifest(&manifest_path)?;
    let Some(workspace) = raw.workspace else {
        return Err(Diagnostic::new(format!(
            "manifest `{}` is missing a [workspace] section",
            manifest_path.display()
        )));
    };

    workspace
        .members
        .into_iter()
        .map(|member| canonicalize_dir(&workspace_root.join(normalize_member_path(&member))))
        .collect()
}

fn load_package_manifest(manifest_dir: &Path) -> Result<ParsedPackageManifest> {
    let manifest_path = manifest_dir.join(MANIFEST_NAME);
    let raw = load_raw_manifest(&manifest_path)?;
    let Some(package) = raw.package else {
        return Err(Diagnostic::new(format!(
            "manifest `{}` is missing a [package] section",
            manifest_path.display()
        )));
    };
    if package.name.trim().is_empty() {
        return Err(Diagnostic::new(format!(
            "manifest `{}` has an empty package name",
            manifest_path.display()
        )));
    }
    if !is_valid_package_name(&package.name) {
        return Err(Diagnostic::new(format!(
            "manifest `{}` has an invalid package name `{}`; package names must match `[A-Za-z_][A-Za-z0-9_]*`",
            manifest_path.display(),
            package.name
        )));
    }
    if package.edition.trim().is_empty() {
        return Err(Diagnostic::new(format!(
            "manifest `{}` has an empty package edition",
            manifest_path.display()
        )));
    }
    if package.version.trim().is_empty() {
        return Err(Diagnostic::new(format!(
            "manifest `{}` has an empty package version",
            manifest_path.display()
        )));
    }
    if !is_valid_package_version(&package.version) {
        return Err(Diagnostic::new(format!(
            "manifest `{}` has an invalid package version `{}`; package versions must start with a digit and contain only ASCII letters, digits, `.`, `-`, or `+`",
            manifest_path.display(),
            package.version
        )));
    }
    if package.edition != SUPPORTED_PACKAGE_EDITION {
        return Err(Diagnostic::new(format!(
            "manifest `{}` uses unsupported package edition `{}`; the current Aurora compiler supports edition `{}`",
            manifest_path.display(),
            package.edition,
            SUPPORTED_PACKAGE_EDITION
        )));
    }
    Ok(ParsedPackageManifest {
        package,
        dependencies: raw.dependencies,
    })
}

fn load_raw_manifest(manifest_path: &Path) -> Result<RawManifest> {
    let source = fs::read_to_string(manifest_path).map_err(|error| {
        Diagnostic::new(format!(
            "failed to read manifest `{}`: {}",
            manifest_path.display(),
            error
        ))
    })?;
    toml::from_str(&source).map_err(|error| {
        Diagnostic::new(format!(
            "failed to parse manifest `{}`: {}",
            manifest_path.display(),
            error
        ))
    })
}

fn load_lockfile(lockfile_root: &Path) -> Result<BTreeMap<String, LockedPackage>> {
    let lockfile_path = lockfile_root.join(LOCKFILE_NAME);
    if !lockfile_path.exists() {
        return Ok(BTreeMap::new());
    }

    let source = fs::read_to_string(&lockfile_path).map_err(|error| {
        Diagnostic::new(format!(
            "failed to read lockfile `{}`: {}",
            lockfile_path.display(),
            error
        ))
    })?;
    let raw: RawLockfile = toml::from_str(&source).map_err(|error| {
        Diagnostic::new(format!(
            "failed to parse lockfile `{}`: {}",
            lockfile_path.display(),
            error
        ))
    })?;

    let version = raw.version.unwrap_or(LOCKFILE_VERSION);
    if version != LOCKFILE_VERSION {
        return Err(Diagnostic::new(format!(
            "unsupported lockfile version `{}` in `{}`; expected version `{}`",
            version,
            lockfile_path.display(),
            LOCKFILE_VERSION
        )));
    }

    let mut packages = BTreeMap::new();
    for package in raw.package {
        let locked = match package.source.as_str() {
            "path" => {
                if package.path.is_none() {
                    return Err(Diagnostic::new(format!(
                        "lockfile entry for package `{}` is missing `path`",
                        package.name
                    )));
                }
                LockedPackage {
                    source: LockedPackageSource::Path,
                }
            }
            "git" => {
                let Some(git) = package.git else {
                    return Err(Diagnostic::new(format!(
                        "lockfile entry for package `{}` is missing `git`",
                        package.name
                    )));
                };
                let Some(rev) = package.rev else {
                    return Err(Diagnostic::new(format!(
                        "lockfile entry for package `{}` is missing `rev`",
                        package.name
                    )));
                };
                let selector =
                    GitSelector::from_lockfile(&package.name, &rev, package.tag, package.branch)?;
                LockedPackage {
                    source: LockedPackageSource::Git {
                        source: git,
                        rev,
                        selector,
                    },
                }
            }
            other => {
                return Err(Diagnostic::new(format!(
                    "lockfile entry for package `{}` has unsupported source `{}`",
                    package.name, other
                )));
            }
        };
        packages.insert(package.name, locked);
    }
    Ok(packages)
}

fn validate_dependency_shape(
    dependency_name: &str,
    path: Option<&str>,
    version: Option<&str>,
    git: Option<&str>,
    rev: Option<&str>,
    tag: Option<&str>,
    branch: Option<&str>,
) -> Result<()> {
    if version.is_some() {
        return Err(unsupported_version_dependency(dependency_name, version));
    }

    let source_count = usize::from(path.is_some()) + usize::from(git.is_some());
    if source_count != 1 {
        return Err(Diagnostic::new(format!(
            "dependency `{}` must choose exactly one dependency source: `path = \"...\"` or `git = \"...\"`",
            dependency_name
        )));
    }

    if git.is_none() && (rev.is_some() || tag.is_some() || branch.is_some()) {
        return Err(Diagnostic::new(format!(
            "dependency `{}` uses `rev`, `tag`, or `branch` without `git = \"...\"`",
            dependency_name
        )));
    }

    if let Some(git) = git {
        validate_git_source_literal(dependency_name, git)?;
    }

    Ok(())
}

fn resolve_git_dependency(
    manifest_dir: &Path,
    dependency_name: &str,
    git_source: String,
    selector: &GitSelector,
    locked: Option<&LockedPackage>,
) -> Result<ResolvedGitDependency> {
    let normalized_source = normalize_git_source(manifest_dir, &git_source)?;
    let resolved_rev = match selector {
        GitSelector::Rev(rev) => rev.clone(),
        _ => {
            match locked.and_then(|package| package.git_locked_rev(&normalized_source, selector)) {
                Some(rev) => rev,
                None => resolve_git_revision(&normalized_source, selector).map_err(|error| {
                    Diagnostic::new(format!(
                        "failed to resolve git dependency `{}` from `{}`: {}",
                        dependency_name, normalized_source, error.message
                    ))
                })?,
            }
        }
    };
    let checkout_dir = ensure_git_checkout(&normalized_source, &resolved_rev).map_err(|error| {
        Diagnostic::new(format!(
            "failed to materialize git dependency `{}` from `{}` at `{}`: {}",
            dependency_name, normalized_source, resolved_rev, error.message
        ))
    })?;
    Ok(ResolvedGitDependency {
        checkout_dir,
        normalized_source,
        resolved_rev,
        selector: selector.clone(),
    })
}

fn resolve_git_revision(source: &str, selector: &GitSelector) -> Result<String> {
    match selector {
        GitSelector::Rev(rev) => Ok(rev.clone()),
        GitSelector::Tag(tag) => {
            let reference = format!("refs/tags/{}", tag);
            let output = run_git_command(
                None,
                vec![
                    "ls-remote".to_string(),
                    "--tags".to_string(),
                    "--refs".to_string(),
                    "--".to_string(),
                    source.to_string(),
                    reference.clone(),
                ],
            )?;
            parse_ls_remote_revision(source, &reference, &output)
        }
        GitSelector::Branch(branch) => {
            let reference = format!("refs/heads/{}", branch);
            let output = run_git_command(
                None,
                vec![
                    "ls-remote".to_string(),
                    "--heads".to_string(),
                    "--".to_string(),
                    source.to_string(),
                    reference.clone(),
                ],
            )?;
            parse_ls_remote_revision(source, &reference, &output)
        }
    }
}

fn parse_ls_remote_revision(source: &str, reference: &str, output: &str) -> Result<String> {
    let Some(line) = output.lines().find(|line| !line.trim().is_empty()) else {
        return Err(Diagnostic::new(format!(
            "git source `{}` does not expose `{}`",
            source, reference
        )));
    };
    let Some(rev) = line.split_whitespace().next() else {
        return Err(Diagnostic::new(format!(
            "could not parse git revision for `{}` from `{}`",
            reference, source
        )));
    };
    validate_git_revision_literal(reference, rev)?;
    Ok(rev.to_string())
}

fn ensure_git_checkout(source: &str, rev: &str) -> Result<PathBuf> {
    let mut permission_denied: Option<(PathBuf, std::io::Error)> = None;
    for cache_root in git_cache_roots() {
        let checkout_dir = cache_root.join(hash_source_key(source)).join(rev);
        if let Ok(metadata) = fs::symlink_metadata(&checkout_dir) {
            let file_type = metadata.file_type();
            if file_type.is_symlink() || !file_type.is_dir() {
                return Err(Diagnostic::new(format!(
                    "refusing to use git checkout cache path `{}` because it is not a real directory",
                    checkout_dir.display()
                )));
            }
        }
        if cached_git_checkout_matches_rev(&checkout_dir, rev)? {
            reject_symlinks_in_tree(&checkout_dir)?;
            return Ok(checkout_dir);
        }

        if checkout_dir.exists() {
            fs::remove_dir_all(&checkout_dir).map_err(|error| {
                Diagnostic::new(format!(
                    "failed to reset git checkout cache `{}`: {}",
                    checkout_dir.display(),
                    error
                ))
            })?;
        }

        let parent = checkout_dir.parent().ok_or_else(|| {
            Diagnostic::new(format!(
                "internal error: git checkout cache path `{}` has no parent",
                checkout_dir.display()
            ))
        })?;
        match fs::create_dir_all(parent) {
            Ok(()) => return materialize_git_checkout(source, rev, &checkout_dir),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied = Some((parent.to_path_buf(), error));
            }
            Err(error) => {
                return Err(Diagnostic::new(format!(
                    "failed to create git cache directory `{}`: {}",
                    parent.display(),
                    error
                )));
            }
        }
    }

    let Some((path, error)) = permission_denied else {
        return Err(Diagnostic::new(
            "internal error: git cache root resolution produced no writable candidates",
        ));
    };
    Err(Diagnostic::new(format!(
        "failed to create git cache directory `{}`: {}",
        path.display(),
        error
    )))
}

fn materialize_git_checkout(source: &str, rev: &str, checkout_dir: &Path) -> Result<PathBuf> {
    let parent = checkout_dir.parent().ok_or_else(|| {
        Diagnostic::new(format!(
            "internal error: git checkout cache path `{}` has no parent",
            checkout_dir.display()
        ))
    })?;
    let temp_checkout = unique_temp_path(parent, rev)?;
    if temp_checkout.exists() {
        let _ = fs::remove_dir_all(&temp_checkout);
    }

    run_git_command(
        None,
        vec![
            "-c".to_string(),
            "core.symlinks=false".to_string(),
            "clone".to_string(),
            "--quiet".to_string(),
            "--".to_string(),
            source.to_string(),
            temp_checkout.to_string_lossy().to_string(),
        ],
    )?;
    reject_symlinks_in_tree(&temp_checkout)?;
    run_git_command(
        Some(&temp_checkout),
        vec![
            "-c".to_string(),
            "advice.detachedHead=false".to_string(),
            "checkout".to_string(),
            "--detach".to_string(),
            rev.to_string(),
        ],
    )?;
    write_cached_git_revision(&temp_checkout, rev)?;

    match fs::rename(&temp_checkout, checkout_dir) {
        Ok(()) => Ok(checkout_dir.to_path_buf()),
        Err(error) if checkout_dir.exists() => {
            let _ = fs::remove_dir_all(&temp_checkout);
            if cached_git_checkout_matches_rev(checkout_dir, rev)? {
                Ok(checkout_dir.to_path_buf())
            } else {
                Err(Diagnostic::new(format!(
                    "failed to place git checkout `{}` because an incompatible cached checkout already exists after rename failed: {}",
                    checkout_dir.display(),
                    error
                )))
            }
        }
        Err(error) => Err(Diagnostic::new(format!(
            "failed to place git checkout `{}`: {}",
            checkout_dir.display(),
            error
        ))),
    }
}

fn git_cache_roots() -> Vec<PathBuf> {
    let primary = git_cache_root();
    let fallback = env::temp_dir().join("aurora").join("git");
    if primary == fallback {
        vec![primary]
    } else {
        vec![primary, fallback]
    }
}

fn configured_git_command(current_dir: Option<&Path>, args: &[String]) -> Command {
    let mut command = Command::new("git");
    command.args(args);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_ASKPASS", "");
    command.env("SSH_ASKPASS", "");
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    command
}

fn git_command_timeout() -> StdDuration {
    env::var("AURORA_GIT_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(StdDuration::from_millis)
        .unwrap_or(DEFAULT_GIT_COMMAND_TIMEOUT)
}

fn pipe_reader_thread<R>(mut reader: R) -> thread::JoinHandle<std::io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).map(|_| output)
    })
}

fn join_command_pipe(
    handle: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    display_name: &str,
    stream_name: &str,
) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| {
            Diagnostic::new(format!(
                "failed to collect `{}` {} output",
                display_name, stream_name
            ))
        })?
        .map_err(|error| {
            Diagnostic::new(format!(
                "failed to read `{}` {} output: {}",
                display_name, stream_name, error
            ))
        })
}

fn run_command_with_timeout(
    mut command: Command,
    display_name: &str,
    timeout: StdDuration,
) -> Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| Diagnostic::new(format!("failed to run `{}`: {}", display_name, error)))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        Diagnostic::new(format!(
            "internal error: `{}` stdout was not captured",
            display_name
        ))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        Diagnostic::new(format!(
            "internal error: `{}` stderr was not captured",
            display_name
        ))
    })?;
    let stdout_handle = pipe_reader_thread(stdout);
    let stderr_handle = pipe_reader_thread(stderr);
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_command_pipe(stdout_handle, display_name, "stdout")?;
                let stderr = join_command_pipe(stderr_handle, display_name, "stderr")?;
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(Diagnostic::new(format!(
                    "failed to wait for `{}`: {}",
                    display_name, error
                )));
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(Diagnostic::new(format!(
                "`{}` timed out after {:?}",
                display_name, timeout
            )));
        }
        thread::sleep(StdDuration::from_millis(10));
    }
}

fn run_git_command(current_dir: Option<&Path>, args: Vec<String>) -> Result<String> {
    let command_display = format!("git {}", args.join(" "));
    let command = configured_git_command(current_dir, &args);
    let output = run_command_with_timeout(command, &command_display, git_command_timeout())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(Diagnostic::new(format!(
            "`{}` failed: {}",
            command_display, details
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn reject_symlinks_in_tree(root: &Path) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let entries = fs::read_dir(&path).map_err(|error| {
            Diagnostic::new(format!(
                "failed to inspect git checkout `{}` for symlinks: {}",
                path.display(),
                error
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                Diagnostic::new(format!(
                    "failed to inspect an entry under git checkout `{}`: {}",
                    path.display(),
                    error
                ))
            })?;
            let file_type = entry.file_type().map_err(|error| {
                Diagnostic::new(format!(
                    "failed to inspect git checkout entry `{}`: {}",
                    entry.path().display(),
                    error
                ))
            })?;
            if file_type.is_symlink() {
                return Err(Diagnostic::new(format!(
                    "refusing to use git checkout `{}` because it contains symlinked content at `{}`",
                    root.display(),
                    entry.path().display()
                )));
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

fn git_cache_root() -> PathBuf {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache_home).join("aurora").join("git");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("aurora")
            .join("git");
    }
    env::temp_dir().join("aurora").join("git")
}

fn normalize_git_source(manifest_dir: &Path, source: &str) -> Result<String> {
    validate_git_source_literal("dependency", source)?;
    if is_explicit_git_url(source) {
        return Ok(source.to_string());
    }

    let candidate = if Path::new(source).is_absolute() {
        PathBuf::from(source)
    } else {
        manifest_dir.join(source)
    };
    if candidate.exists() {
        return canonicalize_dir(&candidate).map(|path| path.to_string_lossy().to_string());
    }
    Err(Diagnostic::new(format!(
        "git dependency source `{}` must be an explicit git URL/SSH source or an existing local path relative to `{}`",
        source,
        manifest_dir.display()
    )))
}

fn is_explicit_git_url(source: &str) -> bool {
    source.contains("://") || source.starts_with("git@")
}

fn hash_source_key(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push_str(&format!("{:02x}", byte));
    }
    encoded
}

fn canonicalize_dir(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|error| {
        Diagnostic::new(format!(
            "failed to resolve package path `{}`: {}",
            path.display(),
            error
        ))
    })
}

fn canonicalize_if_exists(path: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical);
    }

    let mut existing_ancestor = path;
    while !existing_ancestor.exists() {
        let Some(parent) = existing_ancestor.parent() else {
            return Ok(path.to_path_buf());
        };
        existing_ancestor = parent;
    }

    let canonical_ancestor = fs::canonicalize(existing_ancestor).map_err(|error| {
        Diagnostic::new(format!(
            "failed to resolve path `{}`: {}",
            existing_ancestor.display(),
            error
        ))
    })?;
    let Ok(suffix) = path.strip_prefix(existing_ancestor) else {
        return Ok(path.to_path_buf());
    };
    Ok(if suffix.as_os_str().is_empty() {
        canonical_ancestor
    } else {
        canonical_ancestor.join(suffix)
    })
}

fn normalize_member_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return ".".to_string();
    }
    normalize_relative_path(Path::new(trimmed))
}

fn validate_git_source_literal(dependency_name: &str, source: &str) -> Result<()> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an empty git source",
            dependency_name
        )));
    }
    if trimmed.starts_with('-') {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an invalid git source `{}`; git sources must not start with `-`",
            dependency_name, source
        )));
    }
    if trimmed.chars().any(|ch| ch.is_control()) {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an invalid git source `{}`; git sources must not contain control characters",
            dependency_name, source
        )));
    }
    Ok(())
}

fn validate_git_selector_literal(
    dependency_name: &str,
    selector_kind: &str,
    selector: &str,
) -> Result<()> {
    let trimmed = selector.trim();
    if trimmed.is_empty() {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an empty git {}",
            dependency_name, selector_kind
        )));
    }
    if trimmed.starts_with('-') {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an invalid git {} `{}`; git {} values must not start with `-`",
            dependency_name, selector_kind, selector, selector_kind
        )));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an invalid git {} `{}`; git {} values must not contain whitespace or control characters",
            dependency_name, selector_kind, selector, selector_kind
        )));
    }
    if trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.ends_with('.')
        || trimmed.ends_with(".lock")
        || trimmed.contains("..")
        || trimmed.contains("//")
        || trimmed.contains("@{")
        || trimmed.contains('\\')
        || trimmed
            .chars()
            .any(|ch| matches!(ch, '~' | '^' | ':' | '?' | '*' | '['))
    {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an invalid git {} `{}`",
            dependency_name, selector_kind, selector
        )));
    }
    Ok(())
}

fn validate_git_revision_literal(dependency_name: &str, rev: &str) -> Result<()> {
    let trimmed = rev.trim();
    let is_valid_hex_revision =
        (7..=64).contains(&trimmed.len()) && trimmed.chars().all(|ch| ch.is_ascii_hexdigit());
    if !is_valid_hex_revision {
        return Err(Diagnostic::new(format!(
            "dependency `{}` has an invalid git revision `{}`",
            dependency_name, rev
        )));
    }
    Ok(())
}

fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

        unsafe extern "system" {
            fn MoveFileExW(
                existing_file_name: *const u16,
                new_file_name: *const u16,
                flags: u32,
            ) -> i32;
        }

        fn wide(path: &Path) -> Vec<u16> {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        let source_wide = wide(source);
        let destination_wide = wide(destination);
        let moved = unsafe {
            MoveFileExW(
                source_wide.as_ptr(),
                destination_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination)
    }
}

fn unique_temp_path(parent: &Path, file_name: &str) -> Result<PathBuf> {
    for _ in 0..TEMP_FILE_RETRY_LIMIT {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".{}.tmp-{}-{}-{}",
            file_name,
            std::process::id(),
            unix_time_nanos()?,
            counter
        ));
        if !temp_path.exists() {
            return Ok(temp_path);
        }
    }
    Err(Diagnostic::new(format!(
        "failed to create a unique temporary path for `{}` after {} attempts",
        parent.join(file_name).display(),
        TEMP_FILE_RETRY_LIMIT
    )))
}

fn write_atomic_file(path: &Path, contents: &[u8], noun: &str, target: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Diagnostic::new(format!(
            "internal error: {} path `{}` has no parent directory",
            noun,
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Diagnostic::new(format!(
            "failed to prepare parent directory for {} {}: {}",
            noun, target, error
        ))
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("aurora-file");
    let temp_path = unique_temp_path(parent, file_name)?;
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| {
                Diagnostic::new(format!(
                    "failed to create temporary {} for {}: {}",
                    noun, target, error
                ))
            })?;
        file.write_all(contents).map_err(|error| {
            Diagnostic::new(format!(
                "failed to write temporary {} for {}: {}",
                noun, target, error
            ))
        })?;
        file.flush().map_err(|error| {
            Diagnostic::new(format!(
                "failed to flush temporary {} for {}: {}",
                noun, target, error
            ))
        })?;
        replace_file(&temp_path, path).map_err(|error| {
            Diagnostic::new(format!("failed to place {} {}: {}", noun, target, error))
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn is_valid_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_valid_package_version(version: &str) -> bool {
    let mut chars = version.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+'))
}

fn toml_string(value: &str) -> String {
    format!("{:?}", value)
}

fn normalize_relative_path(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir => normalized.push(".."),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        ".".to_string()
    } else {
        normalized.to_string_lossy().replace('\\', "/")
    }
}

fn checked_source_file_path(
    source_root: &Path,
    canonical_source_root: &Path,
    relative_segments: &[String],
) -> Result<PathBuf> {
    let mut path = source_root.to_path_buf();
    for segment in relative_segments {
        path.push(segment);
    }
    path.set_extension("au");
    let canonical = canonicalize_if_exists(&path)?;
    if !canonical.starts_with(canonical_source_root) {
        return Err(Diagnostic::new(format!(
            "resolved import path `{}` escapes package source root `{}`",
            canonical.display(),
            canonical_source_root.display()
        )));
    }
    Ok(canonical)
}

fn cached_git_revision_path(checkout_dir: &Path) -> PathBuf {
    checkout_dir.join(".aurora-cache-rev")
}

fn write_cached_git_revision(checkout_dir: &Path, rev: &str) -> Result<()> {
    write_atomic_file(
        &cached_git_revision_path(checkout_dir),
        rev.as_bytes(),
        "git cache revision marker",
        &format!("`{}`", checkout_dir.display()),
    )
}

fn cached_git_checkout_matches_rev(checkout_dir: &Path, rev: &str) -> Result<bool> {
    if !git_checkout_contains_required_files(checkout_dir)? {
        return Ok(false);
    }
    let Some(cached) = read_cached_git_revision(checkout_dir)? else {
        return Ok(false);
    };
    Ok(cached.trim() == rev)
}

fn git_checkout_contains_required_files(checkout_dir: &Path) -> Result<bool> {
    let manifest_path = checkout_dir.join(MANIFEST_NAME);
    let Ok(metadata) = fs::symlink_metadata(&manifest_path) else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() {
        return Err(Diagnostic::new(format!(
            "refusing to use git checkout `{}` because its manifest is symlinked",
            checkout_dir.display()
        )));
    }
    Ok(metadata.is_file())
}

#[cfg(unix)]
fn open_nofollow_dir_fd(path: &Path) -> Result<Option<OwnedFd>> {
    let display_path = path.display().to_string();
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        Diagnostic::new(format!(
            "refusing to use git checkout path `{}` because it contains an interior NUL byte",
            display_path
        ))
    })?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(Diagnostic::new(format!(
            "failed to inspect git checkout directory `{}`: {}",
            display_path, error
        )));
    }
    Ok(Some(unsafe { OwnedFd::from_raw_fd(fd) }))
}

#[cfg(unix)]
fn open_nofollow_file_at(
    dir_fd: &OwnedFd,
    name: &str,
    checkout_dir: &Path,
) -> Result<Option<fs::File>> {
    let name = CString::new(name).map_err(|_| {
        Diagnostic::new(format!(
            "internal error: git checkout marker `{}` contains an interior NUL byte",
            name
        ))
    })?;
    let fd = unsafe {
        libc::openat(
            dir_fd.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(Diagnostic::new(format!(
            "failed to inspect git checkout marker `{}` under `{}`: {}",
            name.to_string_lossy(),
            checkout_dir.display(),
            error
        )));
    }
    Ok(Some(unsafe { fs::File::from_raw_fd(fd) }))
}

#[cfg(unix)]
fn read_cached_git_revision(checkout_dir: &Path) -> Result<Option<String>> {
    let Some(dir_fd) = open_nofollow_dir_fd(checkout_dir)? else {
        return Ok(None);
    };
    let Some(mut marker) = open_nofollow_file_at(&dir_fd, ".aurora-cache-rev", checkout_dir)?
    else {
        return Ok(None);
    };
    let mut cached = String::new();
    marker.read_to_string(&mut cached).map_err(|error| {
        Diagnostic::new(format!(
            "failed to read git revision marker for `{}`: {}",
            checkout_dir.display(),
            error
        ))
    })?;
    Ok(Some(cached))
}

#[cfg(not(unix))]
fn read_cached_git_revision(checkout_dir: &Path) -> Result<Option<String>> {
    let path = cached_git_revision_path(checkout_dir);
    let Ok(cached) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    Ok(Some(cached))
}

fn unix_time_nanos() -> Result<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| {
            Diagnostic::new(format!(
                "failed to read the system clock while creating a temporary path: {}",
                error
            ))
        })
}

fn unsupported_version_dependency(name: &str, version: Option<&str>) -> Diagnostic {
    let detail = version.unwrap_or("<unspecified>");
    Diagnostic::new(format!(
        "version-only dependencies are not supported yet for `{}` (requested `{}`); use `{} = {{ path = \"...\" }}` or `{} = {{ git = \"...\" }}` instead",
        name, detail, name, name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "{}-{}-{}",
                prefix,
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time should be after unix epoch")
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("failed to create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn configured_git_command_disables_interactive_prompts() {
        let args = vec!["status".to_string()];
        let command = configured_git_command(None, &args);
        let envs = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().to_string(),
                    value.map(|value| value.to_string_lossy().to_string()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            envs.get("GIT_TERMINAL_PROMPT"),
            Some(&Some("0".to_string()))
        );
        assert_eq!(envs.get("GIT_ASKPASS"), Some(&Some(String::new())));
        assert_eq!(envs.get("SSH_ASKPASS"), Some(&Some(String::new())));
    }

    #[cfg(unix)]
    #[test]
    fn command_timeout_terminates_hung_git_helpers() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let error =
            run_command_with_timeout(command, "git test-timeout", StdDuration::from_millis(50))
                .expect_err("hung commands should time out");
        assert!(error.message.contains("timed out"));
        assert!(
            started.elapsed() < StdDuration::from_secs(2),
            "timeout helper should not wait for the child sleep to finish"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reject_symlinks_in_tree_reports_symlinked_entries() {
        let temp = TempDir::new("aurora-package-symlink-tree");
        let root = temp.path.join("checkout");
        fs::create_dir_all(root.join("src")).expect("failed to create checkout root");
        fs::write(root.join("Aurora.toml"), "[package]\nname = \"pkg\"\n").expect("manifest");
        std::os::unix::fs::symlink("/tmp", root.join("src").join("escape"))
            .expect("failed to create symlink");

        let error = reject_symlinks_in_tree(&root).expect_err("symlinked content should fail");
        assert!(error.message.contains("contains symlinked content"));
    }

    #[cfg(unix)]
    #[test]
    fn read_cached_git_revision_rejects_symlinked_markers() {
        let temp = TempDir::new("aurora-package-symlink-marker");
        let root = temp.path.join("checkout");
        fs::create_dir_all(&root).expect("failed to create checkout root");
        fs::write(root.join("Aurora.toml"), "[package]\nname = \"pkg\"\n").expect("manifest");
        let target = temp.path.join("outside.rev");
        fs::write(&target, "1234567").expect("failed to write outside marker");
        std::os::unix::fs::symlink(&target, root.join(".aurora-cache-rev"))
            .expect("failed to create symlinked marker");

        let error =
            read_cached_git_revision(&root).expect_err("symlinked revision marker should fail");
        assert!(error
            .message
            .contains("failed to inspect git checkout marker"));
    }
}

fn relative_path_from(base: &Path, target: &Path) -> PathBuf {
    let base_components = base.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();
    let mut common = 0;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }

    let mut relative = PathBuf::new();
    for _ in common..base_components.len() {
        relative.push("..");
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }

    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    relative
}
