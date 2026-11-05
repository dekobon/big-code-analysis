//! The `.gitattributes` capture the history walk resolves under (#1760,
//! #1812), both halves of it. From the index: the `(path, blob id)`
//! mappings of every `.gitattributes` it holds, computed as gix's own
//! stack computes them, with the root blob withheld so the hand-built
//! stack can place it where gix's first push would. From outside the
//! index: the system, user and `info/attributes` files, read once per
//! open so the diff platform's stack and the cache digest are built from
//! one set of bytes, with the user file asked of gix itself
//! ([`gix::Repository::attribute_global_paths`]) so an `:(optional)`
//! spelling is anchored and checked the way gix anchors and checks it.
//! `history::tree_diff_platform` consumes the capture;
//! [`WalkContext::diff_attributes_digest`](super::history::WalkContext::diff_attributes_digest)
//! says why the digest exists.

use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use gix::bstr::BString;

/// The `(path, blob id)` pairs of every `.gitattributes` the index holds,
/// computed the way gix's `attributes_only` computes the stack's own.
/// Computed once per open and handed both to the stack and to the digest
/// (term 1), so the two cannot read different index loads.
pub(super) fn id_mappings(
    index: &gix::index::State,
    ignore_case: bool,
) -> Vec<(BString, gix::ObjectId)> {
    gix::worktree::stack::State::AttributesStack(gix::worktree::stack::state::Attributes::default())
        .id_mappings_from_index(index, index.path_backing(), glob_case(ignore_case))
}

/// The glob case rule gix derives from `core.ignoreCase`.
pub(super) fn glob_case(ignore_case: bool) -> gix::glob::pattern::Case {
    if ignore_case {
        gix::glob::pattern::Case::Fold
    } else {
        gix::glob::pattern::Case::Sensitive
    }
}

/// The digest [`WalkContext::diff_attributes_digest`](super::history::WalkContext::diff_attributes_digest) documents, over the
/// three inputs [`WalkContext::open`](super::history::WalkContext::open) captured: the id mappings the stack
/// was handed, the attribute-file bytes it was built from, and the case
/// rule it matches under.
pub(super) fn diff_attributes_digest(
    mappings: &[(BString, gix::ObjectId)],
    sources: &AttributeSources,
    ignore_case: bool,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    mappings.len().hash(&mut hasher);
    for (path, id) in mappings {
        path.hash(&mut hasher);
        hasher.write(id.as_slice());
    }
    for (tag, source) in (0u8..).zip(&sources.files) {
        tag.hash(&mut hasher);
        match &source.bytes {
            Some(bytes) => {
                true.hash(&mut hasher);
                bytes.hash(&mut hasher);
            }
            None => false.hash(&mut hasher),
        }
    }
    u8::from(ignore_case).hash(&mut hasher);
    hasher.finish()
}

/// Whether gix's `add_patterns_file` reads an attribute file that failed
/// to open this way as *absent* rather than failing: a missing path, or
/// a path that names a directory. Mirrors gix-glob 0.29's
/// `read_in_full_ignore_missing` and `io_err_is_dir`
/// (`src/search/pattern.rs`), including the Windows clause: there,
/// opening a directory as a file reports `PermissionDenied`, so gix reads
/// that kind as a directory too — and with it a genuine permission
/// failure, which it cannot tell apart. The capture is now the *only*
/// reader of these files, so what it treats as absent is what the walk
/// resolves without, and this rule has to be gix's exactly or an
/// attributes file a user could not read would fail here where `git`
/// and gix silently skip it.
fn gix_treats_as_missing(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::IsADirectory
            | std::io::ErrorKind::NotADirectory
    ) || (cfg!(windows) && err.kind() == std::io::ErrorKind::PermissionDenied)
}

/// The user attribute file gix selected for `repo`, or the XDG default
/// resolved through `env` when `permissions.git` allows one and the
/// open's own permissions did not.
///
/// gix's selection is [`gix::Repository::attribute_global_paths`]: the
/// installation file (never — every preset turns it off), the system
/// file, then `core.attributesFile` or the XDG default, each present
/// only when selected, every one anchored to
/// [`gix::Repository::current_dir`]. The user file is the entry after
/// the system one, when gix selected a system file, else the first —
/// a positional read, so a user file configured *at* the system
/// location is still the user file. Whether gix selected a system
/// file is decided the way gix decides it: the open's
/// `attributes.system` permission and `GIT_ATTR_NOSYSTEM` read under
/// the open's *environment* permissions, which an isolated open
/// denies — `env`, the test seam, is deliberately not consulted here.
/// Asking gix rather than `trusted_path` is what gets an
/// `:(optional)` spelling right: gix anchors the path before the
/// existence check `optional` adds, where `trusted_path` checks it
/// against the process's directory at read time.
///
/// # Errors
///
/// Fails when `core.attributesFile` is set but cannot be interpolated.
fn user_attributes_path(
    repo: &gix::Repository,
    env: &mut dyn FnMut(&str) -> Option<OsString>,
    permissions: gix::open::permissions::Attributes,
) -> gix::Result<Option<PathBuf>> {
    use gix::attrs::Source;
    let base = repo.current_dir();
    let open_permissions = &repo.open_options().permissions;
    let mut gix_env = |name: &str| {
        open_permissions
            .env
            .git_prefix
            .check_opt(name)
            .and_then(gix::path::env::var)
    };
    let gix_system = open_permissions
        .attributes
        .system
        .then(|| Source::System.storage_location(&mut gix_env))
        .flatten()
        .map(|path| base.join(path));
    let selected = repo.attribute_global_paths()?;
    let user = match (selected.first(), gix_system) {
        (Some(first), Some(system)) if *first == system => selected.get(1),
        (first, _) => first,
    };
    Ok(user.cloned().or_else(|| {
        permissions
            .git
            .then(|| Source::Git.storage_location(env))
            .flatten()
            .map(|path| base.join(path))
    }))
}

/// One attribute file gix loads from outside the index: where it was
/// looked for, and its bytes if it was there.
struct AttributeSource {
    /// The path gix would open, or `None` where gix could not resolve a
    /// location for this source (no `HOME`, no git on `PATH`). Kept for
    /// the stack, which records the source path beside each pattern;
    /// the digest never hashes it.
    path: Option<PathBuf>,
    /// The file's content, or `None` for a file gix reads as absent.
    bytes: Option<Vec<u8>>,
}

/// The one read of every attribute file gix loads from outside the index
/// (issue #1760), in the order gix adds them to its global pattern group,
/// which is the order [`diff_attributes_digest`] hashes them in.
///
/// Both consumers — the platform's attribute stack and the digest — are
/// built from one capture, so the walk can only resolve under the bytes
/// the entry is stamped with. Written out against gix 0.90's
/// `attribute_global_paths` (the system and user files, in that
/// order — never the installation's, which every permission preset
/// turns off) and `Attributes::push_directory`
/// (`info/attributes`), so a source gix adds must be added here;
/// [`WalkContext::diff_attributes_digest`](super::history::WalkContext::diff_attributes_digest) says why that failure leans
/// toward staleness.
pub(super) struct AttributeSources {
    /// System, user, `info/attributes`.
    files: [AttributeSource; 3],
}

/// The index entry gix's `push_directory` looks up for the root
/// directory: the exact, case-sensitive spelling its binary search
/// compares against, whatever case rule the mappings were collected
/// under.
const ROOT_ATTRIBUTES: &str = ".gitattributes";

impl AttributeSources {
    /// Read the three sources once, under the permissions gix's own
    /// build applies: the system file only when `permissions.system`,
    /// the XDG default only when `permissions.git`, as
    /// `attribute_global_paths` gates them. `core.attributesFile`
    /// is consulted regardless, as it is there. The installation's
    /// file is never read: every gix preset sets `git_binary: false`,
    /// so gix never loads it either, and resolving its location would
    /// run the `git` binary.
    ///
    /// The user file is the one gix selected ([`user_attributes_path`]):
    /// `core.attributesFile` when set, else the XDG default — resolved
    /// through `env` when `permissions.git` allows it and the open's
    /// own permissions did not. The XDG and system locations come from
    /// [`gix::attrs::Source::storage_location`] with `env`, which
    /// production passes as [`gix::path::env::var`]: the resolver gix's
    /// own `make_source_env` wraps, whose `HOME` falls back to the
    /// platform home directory (`std::env::home_dir`, the passwd entry
    /// on Unix) when the variable is unset. A plain `std::env::var_os`
    /// here read `~/.config/git/attributes` as absent exactly when gix
    /// loaded it. `GIT_ATTR_NOSYSTEM` drops the system file here as it
    /// does there. A location that cannot be resolved, or that the
    /// permissions withhold, is captured as absent.
    ///
    /// Every path is anchored to [`gix::Repository::current_dir`] — the
    /// working directory captured when the repository was opened — as
    /// gix 0.90 anchors its own. A relative `core.attributesFile` or a
    /// relative repository path otherwise resolved against whatever the
    /// process's working directory was at *read* time, so a `chdir`
    /// between open and walk made this capture read a different file
    /// from the one gix's platform resolves.
    ///
    /// # Errors
    ///
    /// Fails when `core.attributesFile` is set but cannot be
    /// interpolated, or when a file exists but cannot be read — the
    /// errors gix's own platform build raises for the same inputs.
    pub(super) fn read(
        repo: &gix::Repository,
        env: &mut dyn FnMut(&str) -> Option<OsString>,
        permissions: gix::open::permissions::Attributes,
    ) -> gix::Result<Self> {
        let base = repo.current_dir();
        let system = permissions
            .system
            .then(|| gix::attrs::Source::System.storage_location(env))
            .flatten()
            .map(|path| base.join(path));
        let user = user_attributes_path(repo, env, permissions)?;
        let mut files = [
            system,
            user,
            Some(base.join(repo.common_dir()).join("info").join("attributes")),
        ]
        .map(|path| AttributeSource { path, bytes: None });
        for file in &mut files {
            file.bytes = match file.path.as_deref().map(std::fs::read) {
                Some(Ok(bytes)) => Some(bytes),
                Some(Err(err)) if !gix_treats_as_missing(&err) => {
                    return Err(gix::Error::from_error(err));
                }
                Some(Err(_)) | None => None,
            };
        }
        Ok(Self { files })
    }

    /// Build the attribute state gix's `attributes_only` would, from the
    /// captured bytes instead of from a read of its own, with
    /// `root_attributes` — the root `.gitattributes` blob, when the index
    /// holds one — folded in where gix's stack would push it.
    ///
    /// # Reproducing gix's precedence from captured bytes
    ///
    /// gix seeds its *global* group with the built-in `binary` macro and
    /// then each file it read, and leaves `info/attributes` to the
    /// stack's first directory push, which appends it to the
    /// *per-directory* group right after the root `.gitattributes`.
    /// Matching walks the per-directory group, then the globals, each
    /// last-added-first, so the order a lookup consults is: a nested
    /// `.gitattributes`, `info/attributes`, the root `.gitattributes`,
    /// the user file, the system file, the built-in.
    /// (That is not git's: `git check-attr` has `info/attributes`
    /// override every in-tree file, nested ones included. The walk has
    /// always resolved through gix's stack, so gix's order is the one
    /// reproduced here, and the parity test says where the two differ.)
    ///
    /// The per-directory group cannot be seeded from outside, so the
    /// captured `info/attributes` has to be a global — and a global
    /// below the root `.gitattributes` unless the root moves too. It
    /// does: the root blob is appended to the globals just before
    /// `info/attributes`, with the base and macro permission gix's push
    /// gives it (none, and allowed, since the root is the directory
    /// macros may be read from), and `tree_diff_platform` withholds
    /// its mapping from the stack so the first push finds nothing and
    /// pushes gix's empty placeholder instead. The group that holds a
    /// list is then different, the order lookups consult is the same,
    /// and so is the order the lists enter the metadata collection.
    /// `the_hand_built_stack_resolves_diff_as_gix_s_does` pins this
    /// against gix's own construction on a fixture where every pair of
    /// sources overlaps.
    pub(super) fn into_attributes(
        self,
        root_attributes: Option<&[u8]>,
        source: gix::worktree::stack::state::attributes::Source,
    ) -> std::io::Result<gix::worktree::stack::state::Attributes> {
        let mut collection = gix::attrs::search::MetadataCollection::default();
        let mut globals = gix::attrs::Search::default();
        globals.add_patterns_buffer(
            b"[attr]binary -diff -merge -text",
            "[builtin]".into(),
            None,
            &mut collection,
            true,
        )?;
        let [system, user, info] = self.files;
        for file in [system, user] {
            let (Some(path), Some(bytes)) = (file.path, file.bytes) else {
                continue;
            };
            globals.add_patterns_buffer(&bytes, path, None, &mut collection, true)?;
        }
        if let Some(bytes) = root_attributes {
            globals.add_patterns_buffer(
                bytes,
                ROOT_ATTRIBUTES.into(),
                None,
                &mut collection,
                true,
            )?;
        }
        if let (Some(path), Some(bytes)) = (info.path, info.bytes) {
            globals.add_patterns_buffer(&bytes, path, None, &mut collection, true)?;
        }
        Ok(gix::worktree::stack::state::Attributes::new(
            globals, None, source, collection,
        ))
    }
}

/// Remove the root `.gitattributes` from `mappings` and return its blob,
/// for [`AttributeSources::into_attributes`] to place among the globals.
/// The lookup is the exact one gix's `push_directory` makes for the root
/// — a binary search on the sorted mappings for the literal
/// [`ROOT_ATTRIBUTES`] — so the entry withheld here is precisely the one
/// the stack would have found. Under `core.ignoreCase` the mappings may
/// also hold a `.GitAttributes`; gix's search misses it just the same,
/// so it stays.
///
/// # Errors
///
/// Fails when the index names a blob the object database does not hold,
/// the failure gix's first push would raise.
pub(super) fn take_root_attributes(
    repo: &gix::Repository,
    mappings: &mut Vec<(BString, gix::ObjectId)>,
) -> gix::Result<Option<Vec<u8>>> {
    let Ok(at) =
        mappings.binary_search_by(|(path, _)| path.as_slice().cmp(ROOT_ATTRIBUTES.as_bytes()))
    else {
        return Ok(None);
    };
    let (_, id) = mappings.remove(at);
    Ok(Some(repo.find_blob(id)?.take_data()))
}

#[cfg(test)]
#[path = "attributes_tests.rs"]
mod tests;
