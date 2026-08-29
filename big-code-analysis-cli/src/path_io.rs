//! File / byte / path I/O helpers for the CLI: stdout-or-file emission,
//! glob-set construction, `--paths-from` / `--exclude-from` line readers,
//! atomic writes, and the config / baseline / preproc loaders.

use super::*;

/// Write every chunk of `parts` to stdout under one lock, flush, and
/// `die` on any failure other than `BrokenPipe` (the typical case when
/// the consumer is `head`, `less`, etc.).
///
/// The single place the stdout-failure policy is decided, so the
/// newline-appending variant below cannot drift from it.
///
/// The flush carries the same `BrokenPipe` exemption as the writes, and
/// is what makes the policy true rather than incidental: `Stdout` is a
/// `LineWriter` over a 1 KiB buffer, so a payload containing no newline
/// *anywhere* in it and shorter than that never reaches the fd here — it
/// goes out in the exit-time cleanup flush, whose error is discarded and
/// the run exits 0 having emitted nothing. That is the shape `bca vcs`
/// shipped with (see [`crate::formats::write_text`]).
///
/// No caller of *this* helper can stage it today: every document `bca`
/// prints through it is either line-oriented or pretty-printed JSON, so
/// the buffer spills on an interior newline and the error surfaces from
/// a `write_all`. The hole is therefore latent rather than live — which
/// is exactly why it needs pinning here instead of end-to-end, and why
/// [`write_parts_flushed`] is a seam.
fn write_stdout_parts_or_die(parts: &[&[u8]]) {
    let mut out = std::io::stdout().lock();
    if let Err(e) = write_parts_flushed(&mut out, parts)
        && e.kind() != ErrorKind::BrokenPipe
    {
        die(e);
    }
}

/// Write every chunk of `parts` to `out` in order, then flush.
///
/// Split out so the flush can be exercised against a sink that accepts
/// every write and fails only at flush time — the shape a `LineWriter`
/// presents to a newline-free payload, and one no shipped subcommand can
/// produce (see [`write_stdout_parts_or_die`]). Without the seam,
/// deleting `out.flush()` fails no test in the workspace.
pub(crate) fn write_parts_flushed(out: &mut impl Write, parts: &[&[u8]]) -> std::io::Result<()> {
    parts.iter().try_for_each(|part| out.write_all(part))?;
    out.flush()
}

/// Write `bytes` to stdout under the policy of
/// [`write_stdout_parts_or_die`].
pub(crate) fn write_stdout_or_die(bytes: &[u8]) {
    write_stdout_parts_or_die(&[bytes]);
}

/// Apply [`write_stdout_parts_or_die`]'s policy to an emission that
/// wrote itself: `die` with `context` on any failure other than
/// `BrokenPipe`.
///
/// The `vcs` family emits through [`crate::formats::write_text`] rather
/// than through the helpers above, and used to `die` on *every* error.
/// Once `write_text` grew the flush that makes a write failure visible
/// at all (#1132), that turned the routine `bca vcs … | head` into an
/// `error: writing vcs output: Broken pipe` and an exit 1, while
/// `dump` / `metrics` / `ops` piped into the same consumer exit 0. A
/// closed consumer is not a tool error on one subcommand and routine on
/// the rest.
///
/// Safe for the `--output <file>` half of those emitters too: a regular
/// file cannot produce `EPIPE`, so the exemption can only fire on the
/// stdout path it is written for.
pub(crate) fn die_unless_broken_pipe(result: std::io::Result<()>, context: &str) {
    if let Err(e) = result
        && e.kind() != ErrorKind::BrokenPipe
    {
        die(format_args!("{context}: {e}"));
    }
}

/// Write `text` and a trailing newline to stdout, under one lock.
///
/// The `println!` that post-walk emissions (`count`'s tally, `preproc`'s
/// JSON) used instead *panics* on a write error, exiting 101 where the
/// CLI documents `EXIT_TOOL_ERROR` (#1132). The newline is passed as a
/// second chunk so both go out under the *one* lock
/// [`write_stdout_parts_or_die`] holds — a parallel walk cannot then
/// split a line from its terminator — and so the `BrokenPipe`-vs-`die`
/// decision and the flush stay in a single place. (An equivalent
/// `writeln!` would be no more costly; it would just re-decide the
/// policy here.)
pub(crate) fn writeln_stdout_or_die(text: &str) {
    write_stdout_parts_or_die(&[text.as_bytes(), b"\n"]);
}

/// Reject an `--output` path that names an existing directory or whose
/// parent directory is missing, mirroring the fast pre-walk validation
/// `report` / `exemptions` do. `label` names the subcommand for the
/// error message.
pub(crate) fn validate_output_path(output: &Path, label: &str) {
    if output.exists() && output.is_dir() {
        die(format_args!("--output must be a file path for `{label}`"));
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        die(format_args!(
            "parent directory of --output does not exist: {}",
            parent.display()
        ));
    }
}

/// Emit `bytes` to the `--output` file when given, else stdout. The
/// universal rule across every emitting subcommand: stdout if `--output`
/// is omitted. `verb` is the action phrase for an I/O error (e.g.
/// `"write diff to"`).
pub(crate) fn write_output_or_stdout(output: Option<&Path>, verb: &str, bytes: &[u8]) {
    match output {
        Some(path) => std::fs::write(path, bytes).unwrap_or_else(|e| die_io(verb, path, e)),
        None => write_stdout_or_die(bytes),
    }
}

pub(crate) fn mk_globset(elems: Vec<String>) -> Result<GlobSet, String> {
    mk_globset_retaining(elems).map(|(set, _)| set)
}

/// [`mk_globset`] plus the subset of `elems` it actually compiled, in
/// compile order, so a `GlobSet::matches` index names the pattern the
/// user wrote. The skip below drops patterns, so the caller's original
/// `Vec` is *not* index-aligned with the set — deriving the retained
/// list anywhere but inside this loop would misattribute every pattern
/// after the first dropped one.
fn mk_globset_retaining(elems: Vec<String>) -> Result<(GlobSet, Vec<String>), String> {
    if elems.is_empty() {
        return Ok((GlobSet::empty(), Vec::new()));
    }

    let mut globset = GlobSetBuilder::new();
    let mut retained = Vec::with_capacity(elems.len());
    for e in elems {
        // Normalise the optional leading `./` so `dir/**` and `./dir/**`
        // compile to the same glob; the match-path side is stripped
        // symmetrically in `WalkFilters::passes` / the `[check.exclude]`
        // filter (#726). The emptiness skip runs *after* the strip so a
        // bare `./` (empty once normalised) is skipped like an empty
        // pattern instead of compiling an empty glob.
        let pattern = walk_seed::strip_dot_slash(&e);
        if pattern.is_empty() {
            continue;
        }
        globset
            .add(Glob::new(pattern).map_err(|err| format!("invalid glob pattern {e:?}: {err}"))?);
        retained.push(e);
    }
    let set = globset
        .build()
        .map_err(|err| format!("failed to build glob set: {err}"))?;
    Ok((set, retained))
}

/// An exclude deny-set paired with the source spelling of each pattern
/// it was compiled from, so a match can be reported *by name* rather
/// than as an anonymous "something excluded this".
///
/// The pairing comes from [`mk_globset_retaining`], which is the only
/// place the compile-time pattern skip lives, so the two halves cannot
/// drift out of index alignment.
pub(crate) struct ExcludeGlobs {
    set: GlobSet,
    patterns: Vec<String>,
}

impl ExcludeGlobs {
    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub(crate) fn is_match(&self, path: impl AsRef<Path>) -> bool {
        self.set.is_match(path)
    }

    /// The first configured pattern `path` matches, or `None`.
    ///
    /// One pattern rather than all of them: the caller reports an
    /// override the user is expected to *act* on by moving the entry to
    /// another surface, and naming the first offender is enough to find
    /// the line. Enumerating every overlapping glob would lengthen the
    /// line without changing the action.
    pub(crate) fn first_match(&self, path: impl AsRef<Path>) -> Option<&str> {
        let idx = *self.set.matches(path).first()?;
        self.patterns.get(idx).map(String::as_str)
    }
}

/// Build an [`ExcludeGlobs`] from inline `patterns` unioned with any
/// read from the `from` file (`.gitignore`-style, `-` for stdin). Dies
/// (exit 1) on a file-read or glob-compile error. Shared by the walker's
/// `--exclude` / `--exclude-from` deny-set and `bca check`'s
/// `--check-exclude` / `--check-exclude-from` gate-exemption set (#378)
/// so the two surfaces union and compile globs identically. `flag` is
/// the originating option name (`--exclude-from` or
/// `--check-exclude-from`), used only to attribute a file-read error to
/// the surface the user actually invoked.
pub(crate) fn build_exclude_globset(
    mut patterns: Vec<String>,
    from: Option<&Path>,
    flag: &str,
) -> ExcludeGlobs {
    if let Some(src) = from {
        patterns.extend(read_exclude_patterns_from(src, flag).unwrap_or_else(|e| die(e)));
    }
    let (set, patterns) = mk_globset_retaining(patterns).unwrap_or_else(|e| die(e));
    ExcludeGlobs { set, patterns }
}

/// Group a resolved file list by basename into the
/// `HashMap<basename, Vec<PathBuf>>` shape that
/// [`big_code_analysis::fix_includes`] consumes to resolve cross-file
/// `#include` directives. Computed from the same file list the workers
/// analyzed, so the grouping always matches the analyzed set — this is
/// what `bca preproc` lost when #489 made the library's directory-walk
/// callback dead (see #495).
pub(crate) fn group_files_by_basename(paths: Vec<PathBuf>) -> HashMap<String, Vec<PathBuf>> {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in paths {
        // Skip non-UTF-8 basenames: the preproc include-resolution map
        // (`guess_file`) keys on the UTF-8 file name, so a lossy key
        // could never be matched by an `#include` directive anyway.
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let key = fname.to_string();
        all_files.entry(key).or_default().push(path);
    }
    all_files
}

/// Load existing preproc JSON for the consumer side. The producer side
/// (`bca preproc`) builds its own `Mutex<PreprocResults>` directly.
pub(crate) fn load_preproc_data(path: &Path) -> Arc<PreprocResults> {
    let data = read_file(path).unwrap_or_else(|e| die_io("read preproc data", path, e));
    let parsed = serde_json::from_slice::<PreprocResults>(&data)
        .unwrap_or_else(|e| die_io("parse preproc JSON from", path, e));
    Arc::new(parsed)
}

/// Read newline-separated paths from `src` (a path on disk or `-`
/// for stdin). Skips blank/whitespace-only lines; `#` is treated as a
/// path character, not a comment. Returns `Err(message)` on I/O
/// failure with the failing line number; the CLI caller translates
/// this into a `die` exit.
pub(crate) fn read_paths_from(src: &Path) -> Result<Vec<PathBuf>, String> {
    // Read raw bytes and split on `\n` rather than going through
    // `BufRead::lines` (which decodes UTF-8 and errors on the first
    // invalid byte). A `--paths-from` list may name files whose paths are
    // not valid UTF-8 — exactly the non-UTF-8 paths the rest of the crate
    // tolerates (the baseline encoder, `handle_path`, the walker all
    // preserve raw bytes). Failing the whole list because one entry has a
    // non-UTF-8 byte is the inconsistency #704 flags. Each line is turned
    // into a `PathBuf` from its raw bytes on Unix (lossless); on other
    // platforms there is no stable byte→OsStr view, so UTF-8 decoding is
    // unavoidable there.
    let label = format!("--paths-from {}", src.display());
    let bytes = read_paths_from_bytes(src).map_err(|e| format!("{label}: {e}"))?;
    Ok(split_path_lines(&bytes))
}

/// Append the `--paths-from` entries to `paths`, exiting with the read
/// error when `src` cannot be read. The one place the seed list grows
/// from that source, shared by the walk (`expand_seed_paths`) and by the
/// `check` gate, which materializes the list ahead of the walk because
/// `-` names stdin and stdin can only be read once (#1306).
pub(crate) fn materialize_paths_from(paths: &mut Vec<PathBuf>, src: Option<&Path>) {
    if let Some(src) = src {
        paths.extend(read_paths_from(src).unwrap_or_else(|e| crate::die(e)));
    }
}

/// Read the entire `--paths-from` source (`-` for stdin, else a file) as
/// raw bytes. Kept separate from the line-splitting so the splitter can
/// be unit-tested on synthetic byte input.
pub(crate) fn read_paths_from_bytes(src: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    if src.as_os_str() == "-" {
        std::io::stdin().lock().read_to_end(&mut buf)?;
    } else {
        std::fs::File::open(src)?.read_to_end(&mut buf)?;
    }
    Ok(buf)
}

/// Split a `--paths-from` byte buffer into one `PathBuf` per non-blank
/// line, preserving non-UTF-8 bytes verbatim on Unix. Newline-delimited;
/// a leading UTF-8 BOM, a trailing `\r` (CRLF input), and ASCII
/// surrounding whitespace are trimmed, but interior bytes are kept
/// untouched so a path containing a non-UTF-8 byte survives. `#` is a
/// path character, not a comment (mirrors the prior `path_pattern_filter`
/// policy).
pub(crate) fn split_path_lines(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let trimmed = trim_ws_and_bom(strip_trailing_cr(line));
            (!trimmed.is_empty()).then(|| os_path_from_bytes(trimmed))
        })
        .collect()
}

/// Drop a single trailing `\r` so a CRLF-terminated `--paths-from` line
/// does not carry the carriage return into the path.
pub(crate) fn strip_trailing_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Trim leading and trailing ASCII whitespace bytes and UTF-8 BOMs from
/// both ends, interleaved (so `<BOM>  /path`, `  <BOM>/path`, and
/// `/path<BOM>` all reduce to `/path`). Interior and non-ASCII bytes are
/// preserved, so a non-UTF-8 path survives the trim. Mirrors the
/// whitespace-and-BOM char class the previous `collect_lines` reader
/// trimmed, but at the byte level so it does not require valid UTF-8.
pub(crate) fn trim_ws_and_bom(mut s: &[u8]) -> &[u8] {
    loop {
        let start = s;
        if let Some(rest) = s.strip_prefix(&UTF8_BOM[..]) {
            s = rest;
        }
        s = s.trim_ascii_start();
        if let Some(rest) = s.strip_suffix(&UTF8_BOM[..]) {
            s = rest;
        }
        s = s.trim_ascii_end();
        // Fixed point: another pass removed nothing.
        if s.len() == start.len() {
            return s;
        }
    }
}

/// Build a `PathBuf` from raw path bytes. On Unix this is lossless (paths
/// are arbitrary byte sequences); on other platforms there is no stable
/// byte→`OsStr` view, so the bytes are decoded as UTF-8 lossily — the
/// non-UTF-8 tolerance is a Unix property, matching where the rest of the
/// crate's byte-preserving path handling applies.
#[cfg(unix)]
pub(crate) fn os_path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
pub(crate) fn os_path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Read newline-separated `--exclude` glob patterns from `src` (a
/// path on disk or `-` for stdin). Blank lines and lines whose first
/// non-whitespace character is `#` (`.gitignore`-style comments) are
/// skipped; surrounding whitespace and any UTF-8 BOM on retained
/// lines are trimmed. Returns `Err(message)` on I/O failure with
/// the path / failing line; the CLI caller translates this into a
/// `die` exit. `flag` names the originating option (`--exclude-from`
/// vs `--check-exclude-from`) so the error points at the surface the
/// user actually used.
pub(crate) fn read_exclude_patterns_from(src: &Path, flag: &str) -> Result<Vec<String>, String> {
    read_lines_from(src, flag, exclude_pattern_filter)
}

/// Retention policy for `--exclude-from` lines: keep the trimmed
/// non-blank, non-`#`-prefixed text as an exclude pattern; otherwise
/// skip. Named so the unit tests can exercise the exact policy the
/// production reader applies instead of mirroring it.
pub(crate) fn exclude_pattern_filter(trimmed: &str) -> Option<String> {
    (!trimmed.is_empty() && !trimmed.starts_with('#')).then(|| trimmed.to_owned())
}

/// Open `src` (a path on disk or `-` for stdin), buffer it, and
/// hand each trimmed non-comment line to `map`. Items the closure
/// returns `Some` for are collected; `None` skips the line. `flag`
/// is the user-facing CLI flag name (e.g. `--paths-from`), included
/// in error messages so users can tell which input failed.
///
/// Returns `Err(message)` on file-open failure or per-line I/O
/// failure rather than calling `die` itself, so unit tests and
/// future non-CLI callers can recover. The CLI wrappers above
/// translate the `Err` into a `die` exit at their layer.
pub(crate) fn read_lines_from<T>(
    src: &Path,
    flag: &str,
    map: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, String> {
    if src.as_os_str() == "-" {
        let label = format!("{flag} -");
        collect_lines(std::io::stdin().lock(), &label, map)
    } else {
        let label = format!("{flag} {}", src.display());
        let f = std::fs::File::open(src).map_err(|e| format!("{label}: {e}"))?;
        collect_lines(std::io::BufReader::new(f), &label, map)
    }
}

/// Drain `reader` line-by-line, trimming surrounding whitespace and
/// any UTF-8 BOMs (leading or trailing), then feeding each result
/// to `map`. Returns `Err(message)` on the first I/O failure, with
/// `label` and the failing line number embedded so the caller can
/// surface which input failed without further context.
///
/// BOM stripping is per-line rather than first-line-only: most
/// lines won't carry a BOM, and `\u{feff}` is not whitespace per
/// `char::is_whitespace`, so a BOM-prefixed pattern (e.g. an editor
/// that saved `.bcaignore` as UTF-8-with-BOM) would otherwise
/// become a literal glob starting with U+FEFF that matches no real
/// path — silently disabling the first exclude. Trimming treats
/// whitespace and BOM as a single character class to handle
/// `\u{feff}  pattern` and `pattern\u{feff}` correctly with one
/// pass — the previous order-sensitive `trim().trim_start_matches`
/// chain corrupted those edge cases.
pub(crate) fn collect_lines<R, T>(
    reader: R,
    label: &str,
    map: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, String>
where
    R: std::io::BufRead,
{
    reader
        .lines()
        .enumerate()
        .filter_map(|(i, r)| match r {
            Ok(line) => {
                map(line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')).map(Ok)
            }
            Err(e) => Some(Err(format!("{label}: read error on line {}: {e}", i + 1))),
        })
        .collect()
}

/// Read `path` and decode it as UTF-8, dying (exit 1) on an I/O or
/// non-UTF-8 error. `label` names the file kind in the diagnostic —
/// e.g. `"threshold config"`, `"baseline"`, `"bca.toml"` — producing
/// `failed to read <label> …` / `failed to decode UTF-8 from <label> …`.
/// Centralizes the read+decode half shared by every config/baseline
/// loader; the caller parses the returned text itself, since the parse
/// step differs per format (TOML schema vs. anchored baseline).
pub(crate) fn read_utf8_file(path: &Path, label: &str) -> String {
    let bytes = read_file(path).unwrap_or_else(|e| die_io(&format!("read {label}"), path, e));
    String::from_utf8(bytes)
        .unwrap_or_else(|e| die_io(&format!("decode UTF-8 from {label}"), path, e))
}

/// Load a `[thresholds]` table from `path`, returning its hard scalar
/// limits and optional `[thresholds.soft]` overrides. On any I/O,
/// parse, or schema error the process dies with exit code 1, keeping
/// exit 2 reserved for the "thresholds exceeded" case.
pub(crate) fn load_threshold_config(path: &Path) -> ParsedThresholds {
    let text = read_utf8_file(path, "threshold config");
    let cfg: ThresholdConfig =
        toml::from_str(&text).unwrap_or_else(|e| die_io("parse threshold config", path, e));
    split_thresholds_table(&cfg.thresholds).unwrap_or_else(|e| die(e))
}

/// Load a baseline file. Same error contract as `load_threshold_config`:
/// any I/O, UTF-8, or schema error dies with exit code 1. The anchor
/// is derived from `path` itself, so the baseline keys are interpreted
/// against the file's own directory. `tolerance` and `fuzzy` configure
/// the qualified-symbol matcher (issue #377).
pub(crate) fn load_baseline(path: &Path, tolerance: usize, fuzzy: bool) -> Baseline {
    let text = read_utf8_file(path, "baseline");
    let anchor = baseline::anchor_for(path);
    Baseline::from_str(&text, &anchor, tolerance, fuzzy)
        .unwrap_or_else(|e| die_io("parse baseline", path, e))
}

/// Write `bytes` to `path` atomically: create the parent directory if
/// needed, write to `<path>.bca-tmp`, then rename. Survives a `kill -9`
/// mid-write — the consumer sees either the previous file or the
/// fully-written new file, never a half-written one.
///
/// The suffix is *appended* to the full path rather than replacing the
/// extension, so a user-supplied path like `foo.tmp` does not collide
/// with the temporary file. On rename failure (e.g. cross-filesystem
/// `EXDEV`, permission denied) the temporary file is removed best-effort
/// before propagating the original error.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".bca-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        // Cleanup is best-effort; if the rename failed the user already
        // has an error to report, and a leftover .bca-tmp removal that
        // fails would only obscure it.
        let _ = std::fs::remove_file(&tmp);
    })
}
