//! Parameter validation and job-directory confinement.
//!
//! Two jobs of the same seller share one offering, one holder and one login. What they do **not**
//! share is a directory. Every path a job names is resolved inside that job's own root, and a
//! path that leaves it is refused — including by way of a symlink, which is why resolution is
//! done with `canonicalize` and not by string prefix.

use crate::config::{ParamKind, SellerToolConfig};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Why a call was refused. One variant per reason so tests can assert the *reason*, not just
/// that something failed — a validator that rejects everything would otherwise look correct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reject {
    UnknownOperation { op: String },
    UnknownParam { op: String, param: String },
    MissingParam { op: String, param: String },
    NotAString { param: String },
    TextTooLong { param: String, len: usize, max: usize },
    ControlCharacter { param: String },
    LooksLikeFlag { param: String },
    ShellMetacharacter { param: String, ch: char },
    NotAChoice { param: String },
    EmptyPath { param: String },
    AbsolutePath { param: String },
    NonNormalComponent { param: String },
    EscapesJobDir { param: String },
    SymlinkedPath { param: String },
    MissingInput { param: String },
    NotARegularFile { param: String },
    OutputParentMissing { param: String },
    BadJobRoot,
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reject::UnknownOperation { op } => {
                write!(f, "operation {op:?} is not in this seller's offering")
            }
            Reject::UnknownParam { op, param } => write!(f, "{op}: parameter {param:?} is not declared"),
            Reject::MissingParam { op, param } => write!(f, "{op}: parameter {param:?} is required"),
            Reject::NotAString { param } => write!(f, "{param}: must be a string"),
            Reject::TextTooLong { param, len, max } => write!(f, "{param}: {len} bytes exceeds max {max}"),
            Reject::ControlCharacter { param } => write!(f, "{param}: control characters are not accepted"),
            Reject::LooksLikeFlag { param } => {
                write!(f, "{param}: a value starting with '-' would read as a flag")
            }
            Reject::ShellMetacharacter { param, ch } => {
                write!(f, "{param}: character {ch:?} is not accepted")
            }
            Reject::NotAChoice { param } => write!(f, "{param}: not one of the declared choices"),
            Reject::EmptyPath { param } => write!(f, "{param}: empty path"),
            Reject::AbsolutePath { param } => write!(f, "{param}: absolute paths are not accepted"),
            Reject::NonNormalComponent { param } => {
                write!(f, "{param}: '.' and '..' components are not accepted")
            }
            Reject::EscapesJobDir { param } => write!(f, "{param}: resolves outside this job's directory"),
            Reject::SymlinkedPath { param } => write!(f, "{param}: symlinks are not accepted"),
            Reject::MissingInput { param } => write!(f, "{param}: no such file in this job's directory"),
            Reject::NotARegularFile { param } => write!(f, "{param}: not a regular file"),
            Reject::OutputParentMissing { param } => write!(f, "{param}: output directory does not exist"),
            Reject::BadJobRoot => write!(f, "job directory is missing or unreadable"),
        }
    }
}

/// A call that passed validation: a fixed subcommand and a fully-formed argv tail. Nothing here
/// is interpreted again downstream — no shell, no string splitting, no template expansion.
#[derive(Clone, Debug)]
pub struct ValidatedCall {
    pub operation: String,
    pub subcommand: String,
    pub argv_tail: Vec<String>,
    pub output_paths: Vec<PathBuf>,
    pub max_output_bytes: usize,
}

/// Characters refused in literal text. The holder never invokes a shell, so this is defence in
/// depth rather than the primary control — kept because "no shell today" is a property of the
/// current code, not of every future edit.
const REFUSED: &[char] = &[
    ';', '|', '&', '$', '`', '<', '>', '(', ')', '{', '}', '[', ']', '*', '?', '!', '\\', '"', '\'',
    '\n', '\r', '\0',
];

pub fn validate_call(
    cfg: &SellerToolConfig,
    operation: &str,
    params: &BTreeMap<String, serde_json::Value>,
    job_root: &Path,
) -> Result<ValidatedCall, Reject> {
    let spec = cfg
        .operation(operation)
        .ok_or_else(|| Reject::UnknownOperation { op: operation.to_string() })?;

    // Every supplied parameter must be declared. Unknown parameters are refused rather than
    // ignored: silently dropping one is how a caller ends up believing a limit was applied.
    for key in params.keys() {
        if !spec.params.iter().any(|p| &p.name == key) {
            return Err(Reject::UnknownParam { op: operation.to_string(), param: key.clone() });
        }
    }

    let root = job_root.canonicalize().map_err(|_| Reject::BadJobRoot)?;

    let mut argv_tail = Vec::new();
    let mut output_paths = Vec::new();

    // Iterate the *spec*, not the input: argv order is fixed by configuration.
    for p in &spec.params {
        let raw = params
            .get(&p.name)
            .ok_or_else(|| Reject::MissingParam { op: operation.to_string(), param: p.name.clone() })?;
        let value = raw.as_str().ok_or_else(|| Reject::NotAString { param: p.name.clone() })?;

        let rendered = match &p.kind {
            ParamKind::Text { max_len } => {
                check_text(&p.name, value, *max_len)?;
                value.to_string()
            }
            ParamKind::Choice { choices } => {
                if !choices.iter().any(|c| c == value) {
                    return Err(Reject::NotAChoice { param: p.name.clone() });
                }
                value.to_string()
            }
            ParamKind::JobInputFile => {
                let path = resolve_job_path(&root, value, &p.name, true)?;
                path.to_string_lossy().into_owned()
            }
            ParamKind::JobOutputFile => {
                let path = resolve_job_path(&root, value, &p.name, false)?;
                output_paths.push(path.clone());
                path.to_string_lossy().into_owned()
            }
        };

        argv_tail.push(p.flag.clone());
        argv_tail.push(rendered);
    }

    Ok(ValidatedCall {
        operation: spec.name.clone(),
        subcommand: spec.subcommand.clone(),
        argv_tail,
        output_paths,
        max_output_bytes: spec.max_output_bytes,
    })
}

fn check_text(param: &str, value: &str, max_len: usize) -> Result<(), Reject> {
    if value.len() > max_len {
        return Err(Reject::TextTooLong { param: param.to_string(), len: value.len(), max: max_len });
    }
    if value.chars().any(|c| c.is_control()) {
        return Err(Reject::ControlCharacter { param: param.to_string() });
    }
    if value.starts_with('-') {
        return Err(Reject::LooksLikeFlag { param: param.to_string() });
    }
    if let Some(ch) = value.chars().find(|c| REFUSED.contains(c)) {
        return Err(Reject::ShellMetacharacter { param: param.to_string(), ch });
    }
    Ok(())
}

/// Resolve `raw` inside the already-canonicalized `root`.
///
/// `must_exist` distinguishes an input (must be there now) from an output (the holder will
/// create it). Both are confined the same way.
pub fn resolve_job_path(
    root: &Path,
    raw: &str,
    param: &str,
    must_exist: bool,
) -> Result<PathBuf, Reject> {
    if raw.is_empty() {
        return Err(Reject::EmptyPath { param: param.to_string() });
    }
    if raw.chars().any(|c| c.is_control()) {
        return Err(Reject::ControlCharacter { param: param.to_string() });
    }

    let rel = Path::new(raw);
    if rel.is_absolute() {
        return Err(Reject::AbsolutePath { param: param.to_string() });
    }
    // Only ordinary names. This is what refuses `../other-job/secret` before any filesystem
    // call happens, and it is intentionally stricter than "no `..` after normalization".
    for c in rel.components() {
        match c {
            Component::Normal(_) => {}
            _ => return Err(Reject::NonNormalComponent { param: param.to_string() }),
        }
    }

    let joined = root.join(rel);

    // A symlink is refused whether or not its target is legal: the check and the later use
    // would otherwise be two different questions.
    if let Ok(md) = std::fs::symlink_metadata(&joined) {
        if md.file_type().is_symlink() {
            return Err(Reject::SymlinkedPath { param: param.to_string() });
        }
    }

    if must_exist {
        let real = joined.canonicalize().map_err(|_| Reject::MissingInput { param: param.to_string() })?;
        if !real.starts_with(root) {
            return Err(Reject::EscapesJobDir { param: param.to_string() });
        }
        if !real.is_file() {
            return Err(Reject::NotARegularFile { param: param.to_string() });
        }
        Ok(real)
    } else {
        let parent = joined.parent().ok_or_else(|| Reject::EmptyPath { param: param.to_string() })?;
        let real_parent = parent
            .canonicalize()
            .map_err(|_| Reject::OutputParentMissing { param: param.to_string() })?;
        if !real_parent.starts_with(root) {
            return Err(Reject::EscapesJobDir { param: param.to_string() });
        }
        let name = joined
            .file_name()
            .ok_or_else(|| Reject::EmptyPath { param: param.to_string() })?;
        Ok(real_parent.join(name))
    }
}
