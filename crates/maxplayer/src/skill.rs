//! `maxplayer skill` — where the agent-facing documentation lives, and the ONE constant every
//! other surface prints that pointer from.
//!
//! Before this module, the whole binary carried exactly one route to the docs: the MCP
//! `initialize.instructions` text in `mcp.rs`. A buyer that registers `maxplayer mcp` was told
//! where the guides are; nobody else ever was — not `maxplayer --help`, not `maxplayer doctor`,
//! not a seller configuring a new seat — and `mcp.rs` itself records that an MCP client may
//! discard `instructions`. An operator whose box never touches MCP (a seller-only seat, or an
//! agent that drives the CLI directly) therefore had no route at all.
//!
//! The URL text is deliberately a single constant, referenced from every path that prints it and
//! asserted against by every test that guards it. Two copies of a URL drift silently — the web
//! build already refuses to keep a second copy of `skill.md` for exactly this reason
//! (`web/app/scripts/build.mjs`, the `/skill.md` alias comment).
//!
//! `maxplayer skill` itself is pure: no home bootstrap, no key, no wallet, no network. It must
//! work on a box that has installed nothing but the binary, because that is the moment an agent
//! needs the pointer.

use std::io::Write;

const SUCCESS: i32 = 0;
const USAGE_ERROR: i32 = 1;

/// The orientation page: the homepage skill, which links every companion skill (buyer, seller,
/// multi-turn buying, debugging, per-box operator notes). Also the URL the MCP handshake points at.
pub const SKILL_URL: &str = "https://www.maxplayer.ai/skill.md";

/// The machine-readable inventory of every published skill: `{name, description, path}` entries.
pub const SKILL_INDEX_URL: &str = "https://www.maxplayer.ai/.well-known/skills/index.json";

/// The one-line pointer shared by `maxplayer --help`, `maxplayer doctor` and the seller first-run
/// path. One function, not three strings, so the pointer cannot be present on one surface and
/// stale on another.
pub fn docs_pointer_line() -> String {
    format!("Docs for agents: {SKILL_URL}  (or run `maxplayer skill`)")
}

/// Entry from `cli::run` for `maxplayer skill`.
///
/// Prints a few lines of plain text an agent can act on: the orientation URL, the skill index, and
/// what to do with them. Any argument other than a sole `--help` is a usage error — there is
/// nothing to configure here, and a flag that is silently ignored teaches the wrong lesson.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // #570: a sole `--help` prints usage to STDOUT and exits 0.
    if crate::cli::is_help_request(args) {
        write_usage(out);
        return SUCCESS;
    }
    if !args.is_empty() {
        write_usage(err);
        return USAGE_ERROR;
    }
    let _ = write!(out, "{}", render());
    SUCCESS
}

/// The text `maxplayer skill` prints. Pure, so the test asserts the exact output.
pub fn render() -> String {
    format!(
        "maxplayer documentation for agents\n\
         \x20 orientation:  {SKILL_URL}\n\
         \x20 skill index:  {SKILL_INDEX_URL}\n\
         Fetch the orientation page first: it links every companion skill (buyer setup, seller \
         setup, multi-turn buying, debugging, and per-box operator notes). The index lists the same \
         skills as machine-readable {{name, description, path}} entries.\n"
    )
}

fn write_usage(out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "Usage:\n  maxplayer skill   # print where the agent documentation lives (orientation URL + skill index); needs no wallet, key or network"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(tail: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = tail.iter().map(|s| (*s).to_owned()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).expect("stdout utf8"),
            String::from_utf8(err).expect("stderr utf8"),
        )
    }

    #[test]
    fn skill_prints_both_urls_to_stdout_and_nothing_else_happens() {
        let (code, out, err) = captured(&[]);
        assert_eq!(code, 0);
        assert!(err.is_empty(), "stderr must stay empty:\n{err}");
        assert_eq!(out, render());
        assert!(out.contains(SKILL_URL), "orientation URL missing:\n{out}");
        assert!(
            out.contains(SKILL_INDEX_URL),
            "skill index URL missing:\n{out}"
        );
        // Plain text an agent can act on: the two URLs each sit on their own line.
        assert!(
            out.lines()
                .any(|line| line.trim_start().starts_with("orientation:"))
        );
        assert!(
            out.lines()
                .any(|line| line.trim_start().starts_with("skill index:"))
        );
    }

    #[test]
    fn skill_index_url_is_under_the_same_origin_as_the_orientation_page() {
        // The index is derived by the web build from the same tree that publishes /skill.md; a
        // pointer to some other origin would send an agent to an inventory that is not this one.
        let origin = "https://www.maxplayer.ai/";
        assert!(SKILL_URL.starts_with(origin));
        assert!(SKILL_INDEX_URL.starts_with(origin));
    }

    #[test]
    fn docs_pointer_line_carries_the_shared_url_and_names_the_subcommand() {
        let line = docs_pointer_line();
        assert!(line.contains(SKILL_URL));
        assert!(line.contains("maxplayer skill"));
        assert!(!line.contains('\n'), "the pointer is ONE line: {line:?}");
    }

    #[test]
    fn skill_help_prints_usage_to_stdout_and_stray_arguments_are_refused() {
        let (code, out, err) = captured(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("Usage:") && out.contains("maxplayer skill"));
        assert!(err.is_empty());

        let (code, out, err) = captured(&["--verbose"]);
        assert_eq!(code, 1);
        assert!(out.is_empty());
        assert!(err.contains("Usage:"));
    }
}
