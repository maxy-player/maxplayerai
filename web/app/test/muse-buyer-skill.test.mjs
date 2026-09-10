/**
 * The maxplayer-muse-buyer skill's shipped invariants: that the bundle installs from
 * its own manifest into an empty home with every link resolving, that it leaks no
 * operator identity, and that every tool-call example it publishes validates against
 * the MCP schema in THIS tree's source.
 *
 *   node --test web/app/test/muse-buyer-skill.test.mjs
 *
 * Node builtins only. No network, no relay, no mint, no daemon, no sats, and no Muse
 * account: this is a bundle-and-schema gate, never an acceptance run. The clean-account
 * Muse acceptance gate is separate and, at the time of writing, unpassed — see the
 * skill's references/verification.md.
 */
import assert from "node:assert/strict";
import {
  copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import test, { after } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");   // web/app
const REPO = resolve(root, "..", "..");                             // repository root
const SKILLS_DIR = join(root, ".well-known", "skills");
const BUYER_DIR = join(SKILLS_DIR, "muse-buyer");
const SKILL_FILE = join(BUYER_DIR, "skill.md");
const MCP_SOURCE = join(REPO, "crates", "maxplayer", "src", "mcp.rs");
const BUYER_SOURCE = join(REPO, "crates", "maxplayer-core", "src", "buyer", "mod.rs");

const temps = [];
function tempRoot(label) {
  const dir = mkdtempSync(join(tmpdir(), `muse-buyer-${label}-`));
  temps.push(dir);
  return dir;
}
after(() => {
  for (const dir of temps) rmSync(dir, { recursive: true, force: true });
});

const skillText = () => readFileSync(SKILL_FILE, "utf8");

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

// --- the shipped bundle ------------------------------------------------------

/** The files the skill's own install manifest tells a reader to save. */
function installManifest() {
  const text = skillText();
  const marker = text.indexOf("<!-- install-manifest -->");
  assert.notEqual(marker, -1, "the skill must publish an install manifest");
  const fence = /```[a-z]*\n([\s\S]*?)```/.exec(text.slice(marker));
  assert.ok(fence, "the install manifest must be a fenced block");
  return fence[1].split("\n").map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
}

test("the install manifest names every file that is shipped, and ships every file it names", () => {
  const manifest = new Set(installManifest());
  for (const entry of manifest) {
    assert.equal(existsSync(join(BUYER_DIR, entry)), true,
      `the manifest names ${entry}, which is not shipped`);
  }
  const shipped = walk(BUYER_DIR).map((file) => relative(BUYER_DIR, file));
  for (const file of shipped) {
    assert.ok(manifest.has(file),
      `${file} is shipped but missing from the install manifest, so a reader's copy would lack it`);
  }
});

test("a fresh-home install carries every link the skill follows, with nothing left behind", () => {
  const home = tempRoot("freshhome");
  const installed = join(home, "workspace", "skills", "muse-buyer");
  for (const entry of installManifest()) {
    const target = join(installed, entry);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(BUYER_DIR, entry), target);
  }
  // Every relative markdown link in every installed file resolves inside the install.
  for (const file of walk(installed)) {
    if (!file.endsWith(".md")) continue;
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(/\]\(([^)\s#][^)\s]*)\)/g)) {
      const href = match[1];
      if (/^(https?:|mailto:)/.test(href)) continue;
      assert.equal(href.startsWith("/"), false,
        `${relative(installed, file)} links to the website path ${href}, which does not resolve from an installed copy`);
      assert.equal(existsSync(resolve(dirname(file), href)), true,
        `${relative(installed, file)} links to ${href}, which the install does not contain`);
    }
  }
});

test("the installed skill states its prerequisites and assumes no working directory", () => {
  const text = skillText();
  assert.match(text, /## Prerequisites/, "a reader must be told what they need before step 1");
  assert.match(text, /maxplayer --version/, "the version check is the first prerequisite");
  // No command may depend on the reader standing in a particular directory.
  for (const match of text.matchAll(/^\s*(?:cd|\.\/)\s.*$/gm)) {
    assert.fail(`the skill assumes a working directory: ${match[0].trim()}`);
  }
});

test("nothing shipped carries an operator identity, home path, key, invoice or balance", () => {
  const forbidden = [
    [/\/Users\/[a-z]+\//i, "an absolute home path"],
    [/\/home\/[a-z]+\//i, "an absolute home path"],
    [/\b(?:npub|nsec)1[02-9ac-hj-np-z]{20,}/i, "a nostr key"],
    [/\b[0-9a-f]{64}\b/i, "a 64-hex key or event id"],
    [/\blnbc[0-9a-z]{20,}/i, "a lightning invoice"],
    [/\bbalance[^.\n]{0,20}\b\d{3,}\s*sats?\b/i, "a wallet balance"],
  ];
  for (const file of walk(BUYER_DIR)) {
    const text = readFileSync(file, "utf8");
    for (const [pattern, what] of forbidden) {
      assert.equal(pattern.test(text), false, `${relative(root, file)} leaks ${what}`);
    }
  }
});

test("the operational core stays inside the 10000-character reusable-skill budget", () => {
  const size = skillText().length;
  assert.ok(size < 10000,
    `skill.md is ${size} characters; branch detail belongs in references/`);
});

test("the skill pins the version it was checked against, and does not silently reuse an older one", () => {
  const cargo = readFileSync(join(REPO, "Cargo.toml"), "utf8");
  const version = /^version\s*=\s*"([0-9.]+)"/m.exec(cargo)[1];
  assert.match(skillText(), new RegExp(`0\\.5\\.8|${version.replace(/\./g, "\\.")}`),
    `the skill must pin the version of the source it was checked against (${version})`);
  for (const file of walk(BUYER_DIR)) {
    const text = readFileSync(file, "utf8");
    if (!/0\.5\.5/.test(text)) continue;
    assert.fail(`${relative(root, file)} still cites 0.5.5, which is not this tree's version`);
  }
});

// --- money discipline the skill must keep --------------------------------------

test("the skill gets a human amount before the single funding invoice, and asks once", () => {
  const text = skillText();
  const setups = [...text.matchAll(/maxplayer wallet setup/g)];
  assert.equal(setups.length, 1,
    "wallet setup appears more than once: a reader could print a second invoice");
  assert.match(text, /21 sats/,
    "omitting the amount silently requests 21 sats; the skill must say so");
  assert.match(text, /mint-complete/, "the second funding step must be shown");
  const source = readFileSync(join(REPO, "crates", "maxplayer", "src", "wallet_cli.rs"), "utf8");
  assert.match(source, /SETUP_FUND_SATS: u64 = 21/,
    "this tree's default funding amount is no longer 21 sats; the skill is stale");
});

test("the skill demands a fresh human approval per paid post and offers no standing budget", () => {
  const text = skillText();
  assert.match(text, /fresh spend/i, "a re-post must be named as a fresh spend");
  assert.match(text, /fresh yes|fresh approval/i, "a re-post must need a fresh approval");
  assert.match(text, /never a standing budget|not a standing budget/i,
    "approval must be scoped to one post");
});

test("the skill never promises that a failed command cost nothing", () => {
  const text = `${skillText()}\n${readFileSync(join(BUYER_DIR, "references", "settlement.md"), "utf8")}`;
  // Written without newline sensitivity: a wrapped sentence is the same promise.
  const flat = text.replace(/\s+/g, " ");
  for (const forbidden of [
    /a refusal costs nothing/i,
    /never charges twice/i,
    /no money (?:has )?moved/i,
    /guaranteed not to (?:charge|spend)/i,
  ]) {
    assert.equal(forbidden.test(flat), false,
      `the buyer pages must not carry a blanket no-charge assurance (${forbidden})`);
  }
  assert.match(flat, /paid/i);
  assert.match(flat, /idempotent|reconcil/i,
    "the recovery from a failed collect must be stated");
});

test("background settlement is documented, since money can move with no collect call", () => {
  const settlement = readFileSync(join(BUYER_DIR, "references", "settlement.md"), "utf8")
    .replace(/\s+/g, " ");
  assert.match(settlement, /background/i);
  assert.match(settlement, /without you (?:ever )?calling `?collect|without you/i);
  assert.match(readFileSync(BUYER_SOURCE, "utf8"), /watcher|settle/i,
    "this tree's buyer no longer shows a background settlement path; the reference is stale");
});

test("an unsupported network route is named as a blocker and no bypass is published", () => {
  const text = skillText().replace(/\s+/g, " ");
  assert.match(text, /relay is not reachable|cannot reach the relay/i,
    "the unreachable-relay case must be named");
  for (const bypass of [/\/etc\/hosts/i, /tunnel/i, /mount namespace/i, /proxy it does not support/i]) {
    if (!bypass.test(text)) continue;
    // Mentioning it is allowed only as a prohibition, never as a recipe.
    const line = text.split("\n").find((candidate) => bypass.test(candidate));
    assert.match(line, /not|never|do not|refuse|blocker/i,
      `${line.trim()} reads as a workaround rather than a prohibition`);
  }
});

// --- published examples against this tree's schema ----------------------------

/** One MCP tool's declared schema, read straight out of the Rust source. */
function toolSchema(name) {
  const source = readFileSync(MCP_SOURCE, "utf8");
  const start = source.indexOf(`"name": "${name}"`);
  assert.notEqual(start, -1, `${name} is not declared in mcp.rs`);
  const end = source.indexOf('"additionalProperties"', start);
  assert.notEqual(end, -1, `${name} has no additionalProperties marker`);
  const block = source.slice(start, end);
  const required = /"required":\s*\[([^\]]*)\]/.exec(block);
  const properties = new Set();
  const propertiesAt = block.indexOf('"properties"');
  if (propertiesAt !== -1) {
    for (const match of block.slice(propertiesAt)
      .matchAll(/"([a-z_]+)":\s*\{\s*"(?:type|description|items|enum)"/gs)) {
      properties.add(match[1]);
    }
  }
  return {
    properties,
    required: required
      ? required[1].split(",").map((part) => part.trim().replace(/"/g, "")).filter(Boolean)
      : [],
  };
}

/** Every {"tool","arguments"} example the skill publishes. */
function publishedExamples() {
  const examples = [];
  for (const match of skillText().matchAll(/```json\n([\s\S]*?)```/g)) {
    let parsed;
    try {
      parsed = JSON.parse(match[1]);
    } catch {
      assert.fail(`the skill publishes a json block that does not parse: ${match[1].slice(0, 80)}`);
    }
    if (parsed && typeof parsed === "object" && parsed.tool) examples.push(parsed);
  }
  return examples;
}

test("every published example validates against the schema in this tree's source", () => {
  const examples = publishedExamples();
  assert.ok(examples.length >= 4, "the skill should show its calls, not describe them");
  const shown = new Set(examples.map((example) => example.tool));
  for (const tool of ["post_job", "get_job", "collect", "award_claim"]) {
    assert.ok(shown.has(tool), `${tool} is part of the buyer surface and must be shown`);
  }
  for (const example of examples) {
    const schema = toolSchema(example.tool);
    const args = example.arguments || {};
    for (const key of Object.keys(args)) {
      assert.ok(schema.properties.has(key),
        `${example.tool} example passes ${key}, which this source's schema does not declare`);
    }
    for (const key of schema.required) {
      assert.ok(key in args, `${example.tool} example omits required argument ${key}`);
    }
  }
});

test("every post_job example carries output, since a post without it is refused", () => {
  for (const example of publishedExamples().filter((one) => one.tool === "post_job")) {
    assert.ok(typeof example.arguments.output === "string" && example.arguments.output.length > 0,
      "post_job requires output; an example without it teaches a refused call");
  }
});

test("the free-job example obeys the enforcement this tree actually performs", () => {
  const free = publishedExamples().filter(
    (one) => one.tool === "post_job" && (one.arguments || {}).payment === "none");
  assert.ok(free.length >= 1, "the free route must be shown as the schema declares it");
  for (const example of free) {
    assert.equal(example.arguments.amount_sats, 0,
      "payment=none with a non-zero amount is refused by the buyer path");
  }
  const source = readFileSync(BUYER_SOURCE, "utf8");
  assert.match(source, /payment=none requires amount_sats = 0/,
    "this tree no longer enforces the free-job rule the skill teaches");
  assert.match(source, /post_job payment must be/,
    "this tree no longer validates the payment vocabulary the skill teaches");
});

test("the skill separates the declared schema from what the server actually enforces", () => {
  const text = `${skillText()}\n${readFileSync(join(BUYER_DIR, "references", "verification.md"), "utf8")}`;
  assert.match(text, /additionalProperties/,
    "the declared strictness must be named so it is not mistaken for enforcement");
  assert.match(text, /deny_unknown_fields/,
    "the absence of runtime unknown-field rejection must be stated");
  assert.equal(/deny_unknown_fields/.test(readFileSync(BUYER_SOURCE, "utf8")), false,
    "this tree now denies unknown fields; the skill's caveat is stale and must be revisited");
});

test("the manual award example carries both required arguments and explains its ceiling", () => {
  const awards = publishedExamples().filter((one) => one.tool === "award_claim");
  assert.ok(awards.length >= 1, "manual award is the fine-grained spend and must be shown");
  for (const award of awards) {
    for (const key of ["job_id", "claim_id"]) {
      assert.ok(key in (award.arguments || {}), `award_claim example omits ${key}`);
    }
  }
  assert.match(skillText(), /write-once/i, "the write-once award must be stated");
});

test("the skill records that MAXPLAYER_HOME is refused as a flag, matching this tree's CLI", () => {
  const text = skillText();
  assert.match(text, /MAXPLAYER_HOME/);
  assert.match(text, /refus/i, "the skill must say --home is refused, not silently ignored");
  const cli = readFileSync(join(REPO, "crates", "maxplayer", "src", "cli.rs"), "utf8");
  assert.match(cli, /buyer_serve_with_home_flag_refuses_instead_of_silently_ignoring_it/,
    "this tree no longer refuses --home; the skill is stale");
});

// --- discovery ----------------------------------------------------------------

const index = JSON.parse(readFileSync(join(SKILLS_DIR, "index.json"), "utf8"));

test("the buyer skill is published in the discovery index and every prior entry survives", () => {
  const names = index.skills.map((entry) => entry.name);
  assert.ok(names.includes("maxplayer-muse-buyer"), "the buyer skill must be indexed");
  for (const name of ["maxplayer-marketplace", "maxplayer-buyer-operate",
    "maxplayer-seller-operate", "maxplayer-multi-turn-buying", "maxplayer-debug-buying",
    "maxplayer-debug-selling", "maxplayer-grok-bot-operate"]) {
    assert.ok(names.includes(name), `${name} must survive in the index`);
  }
  assert.equal(new Set(names).size, names.length, "no duplicate index entries");
  assert.equal(names.includes("maxplayer-muse-seller"), false,
    "the seller skill is out of this branch's scope and must not be indexed here");
});

test("every indexed skill resolves to a file whose frontmatter agrees with the index", () => {
  for (const entry of index.skills) {
    const file = join(root, entry.path.replace(/^\//, ""));
    assert.equal(existsSync(file), true, `${entry.name} points at a missing file`);
    const match = /^---\n([\s\S]*?)\n---/.exec(readFileSync(file, "utf8"));
    assert.ok(match, `${entry.name} has no YAML frontmatter for a client to install by`);
    const fields = {};
    for (const line of match[1].split("\n")) {
      const field = /^([a-z_]+):\s*(.*)$/.exec(line);
      if (field) fields[field[1]] = field[2].replace(/^["']|["']$/g, "");
    }
    assert.equal(fields.name, entry.name, `${entry.name} frontmatter disagrees with the index`);
    assert.ok(fields.description && fields.description.length > 20,
      `${entry.name} needs a description a client can match on`);
  }
});

test("the skill is honest that no clean-account Muse acceptance has been run", () => {
  const verification = readFileSync(join(BUYER_DIR, "references", "verification.md"), "utf8");
  assert.match(verification, /unpassed|not verified|no clean-account/i,
    "the open acceptance gate must be stated, not implied");
  assert.match(skillText(), /UNPROVEN|unpassed|verification\.md/,
    "the core must point at the verification tiers");
});
