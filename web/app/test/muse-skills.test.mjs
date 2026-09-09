/**
 * The Muse skills' shipped invariants, and the behaviour of the helper they
 * bundle. Node builtins only, no network, no seller, no relay, no spend — this
 * file is the offline acceptance gate for the muse-buyer / muse-seller skills:
 *
 *   node --test web/app/test/muse-skills.test.mjs
 *
 * (`npm test` in web/app also picks it up via the test/*.test.mjs glob.)
 *
 * The bridge tests drive the real script over real pipes with a real queue on
 * disk. Nothing is mocked, because every defect these tests exist to catch was a
 * step that passed without exercising its path.
 */
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test, { after } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const BRIDGE = join(root, ".well-known", "skills", "muse-seller", "bin", "muse-acp-bridge.py");

// python3 is already a build-time assumption of this package (see the `serve`
// script in package.json). A missing interpreter FAILS here rather than skipping:
// a green run that never executed the bridge would be exactly the false pass the
// seller's own pre-advertise gate exists to prevent.
const PYTHON = "python3";
assert.equal(
  spawnSync(PYTHON, ["--version"]).status,
  0,
  `${PYTHON} must be runnable to exercise the bundled seller bridge`,
);

const temps = [];
function tempRoot(label) {
  const dir = mkdtempSync(join(tmpdir(), `muse-${label}-`));
  temps.push(dir);
  return dir;
}
after(() => {
  for (const dir of temps) rmSync(dir, { recursive: true, force: true });
});

/** Run a bridge subcommand (claim / done / reap / selfcheck) against one queue. */
function bridge(args, { queue, env = {} } = {}) {
  const result = spawnSync(PYTHON, [BRIDGE, ...args], {
    encoding: "utf8",
    env: { ...process.env, ...(queue ? { MAXPLAYER_MUSE_QUEUE: queue } : {}), ...env },
  });
  assert.equal(result.status, 0, `bridge ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout.trim();
}

/** A live ACP session against `serve`, speaking line-delimited JSON-RPC. */
function startBridge({ queue, env = {} } = {}) {
  const child = spawn(PYTHON, [BRIDGE, "serve"], {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, MAXPLAYER_MUSE_QUEUE: queue, ...env },
  });
  const responses = new Map();
  const waiters = new Map();
  let buffer = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let index;
    while ((index = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line) continue;
      const message = JSON.parse(line);
      if (message.id === undefined) continue; // session/update notification
      responses.set(message.id, message);
      const waiter = waiters.get(message.id);
      if (waiter) {
        waiters.delete(message.id);
        waiter(message);
      }
    }
  });

  let nextId = 1;
  const call = (method, params) => {
    const id = nextId++;
    const promise = new Promise((resolve) => {
      const existing = responses.get(id);
      if (existing) resolve(existing);
      else waiters.set(id, resolve);
    });
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    return promise;
  };
  const notify = (method, params) =>
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);

  return { child, call, notify, stop: () => child.kill() };
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Wait for the queue to hold one job directory and return it. */
async function waitForJob(queue, { timeoutMs = 5000 } = {}) {
  const jobs = join(queue, "jobs");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (existsSync(jobs)) {
      const entries = readdirSync(jobs);
      if (entries.length > 0) return join(jobs, entries[0]);
    }
    await sleep(20);
  }
  throw new Error("no job was published within the timeout");
}

function workdirFor(label) {
  const dir = join(tempRoot(label), "work");
  mkdirSync(dir, { recursive: true });
  return dir;
}

const promptBlocks = (text) => ({ prompt: [{ type: "text", text }] });

test("the bundled bridge passes its own offline selfcheck", () => {
  assert.equal(bridge(["selfcheck"]), "selfcheck ok");
});

test("initialize advertises the protocol version the seller's driver negotiates", async () => {
  const queue = tempRoot("init");
  const acp = startBridge({ queue });
  const reply = await acp.call("initialize", { protocolVersion: 2, clientCapabilities: {} });
  // crates/maxplayer-core/src/driver/acp.rs: PROTOCOL_VERSION = 2, and
  // supports_negotiated_protocol accepts 1..=2.
  assert.equal(reply.result.protocolVersion, 2);
  assert.equal(reply.result.agentCapabilities.loadSession, false);
  acp.stop();
});

test("session/new refuses to guess a workdir instead of falling back to the process cwd", async () => {
  const queue = tempRoot("nocwd");
  const acp = startBridge({ queue });
  await acp.call("initialize", {});

  const missing = await acp.call("session/new", {});
  assert.ok(missing.error, "a session with no cwd must fail, not inherit the seller's cwd");
  assert.match(missing.error.message, /never\s+guesses|absolute cwd/);

  const relative = await acp.call("session/new", { cwd: "relative/path" });
  assert.ok(relative.error, "a relative cwd must fail");

  const absent = await acp.call("session/new", { cwd: join(queue, "does-not-exist") });
  assert.ok(absent.error, "a cwd that does not exist must fail");
  acp.stop();
});

test("session/prompt for an unknown session is refused, not run somewhere else", async () => {
  const queue = tempRoot("identity");
  const acp = startBridge({ queue });
  await acp.call("initialize", {});
  await acp.call("session/new", { cwd: workdirFor("identity-work") });
  const reply = await acp.call("session/prompt", {
    sessionId: "not-a-session-this-process-opened",
    ...promptBlocks("do the thing"),
  });
  assert.ok(reply.error);
  assert.match(reply.error.message, /unknown sessionId/);
  acp.stop();
});

test("a queued turn completes only when the worker reports that turn done", async () => {
  const queue = tempRoot("happy");
  const workdir = workdirFor("happy-work");
  const acp = startBridge({ queue, env: { MAXPLAYER_MUSE_POLL_SECS: "0.05" } });
  await acp.call("initialize", {});
  const session = await acp.call("session/new", { cwd: workdir });

  const turn = acp.call("session/prompt", {
    sessionId: session.result.sessionId,
    ...promptBlocks("Write a haiku into haiku.txt"),
  });
  const jobDir = await waitForJob(queue);
  const meta = JSON.parse(readFileSync(join(jobDir, "meta.json"), "utf8"));
  assert.equal(meta.workdir, workdir, "the job carries the SESSION cwd, not the bridge's cwd");
  assert.equal(meta.kind, "task");
  assert.equal(readFileSync(join(jobDir, "task.md"), "utf8"), "Write a haiku into haiku.txt");

  // The worker side, exactly as the skill instructs a Muse worker run to behave.
  const claim = JSON.parse(bridge(["claim"], { queue }));
  assert.equal(claim.job_dir, jobDir);
  assert.equal(claim.workdir, workdir);
  writeFileSync(join(claim.workdir, "haiku.txt"), "an old silent pond\n");
  bridge(["done", "--job", claim.job_dir, "--status", "ok", "--summary", "wrote haiku.txt"], { queue });

  const reply = await turn;
  assert.equal(reply.result.reason, "completed");
  acp.stop();
});

test("the pre-advertise probe is queued for the real worker, never answered inline", async () => {
  const queue = tempRoot("probe");
  const workdir = workdirFor("probe-work");
  const acp = startBridge({ queue, env: { MAXPLAYER_MUSE_POLL_SECS: "0.05" } });
  await acp.call("initialize", {});
  const session = await acp.call("session/new", { cwd: workdir });

  const sentinel = "maxplayer-probe-hatch-1-1788920000-000000042";
  const turn = acp.call("session/prompt", {
    sessionId: session.result.sessionId,
    ...promptBlocks(
      `Create a file named \`probe.txt\` in your current working directory whose contents are exactly this line:\n\n${sentinel}\n\nDo nothing else.`,
    ),
  });

  const jobDir = await waitForJob(queue);
  const meta = JSON.parse(readFileSync(join(jobDir, "meta.json"), "utf8"));
  assert.equal(meta.kind, "probe", "the probe is labelled so the worker answers it first");
  // The load-bearing assertion: the bridge has NOT written the sentinel itself.
  // If it had, a seat would advertise while its worker path was dead.
  assert.equal(existsSync(join(workdir, "probe.txt")), false,
    "the bridge must not satisfy the seller's readiness probe on the worker's behalf");

  const claim = JSON.parse(bridge(["claim"], { queue }));
  writeFileSync(join(claim.workdir, "probe.txt"), `${sentinel}\n`);
  bridge(["done", "--job", claim.job_dir, "--status", "ok", "--summary", "probe written"], { queue });

  const reply = await turn;
  assert.equal(reply.result.reason, "completed");
  assert.equal(readFileSync(join(workdir, "probe.txt"), "utf8").trim(), sentinel);
  acp.stop();
});

test("two worker runs racing for one job: exactly one claim wins", () => {
  const queue = tempRoot("overlap");
  const workdir = workdirFor("overlap-work");
  const python = [
    "import sys, time",
    `sys.path.insert(0, ${JSON.stringify(dirname(BRIDGE))})`,
    "import importlib.util",
    `spec = importlib.util.spec_from_file_location('bridge', ${JSON.stringify(BRIDGE)})`,
    "mod = importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)",
    `root = ${JSON.stringify(queue)}`,
    `mod.publish_job(root, turn_id='race', session_id='s', workdir=${JSON.stringify(workdir)}, task='t', kind='task', now=time.time())`,
    "wins = [mod.claim_job(root) for _ in range(5)]",
    "print(sum(1 for w in wins if w))",
  ].join("\n");
  const result = spawnSync(PYTHON, ["-c", python], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), "1", "a published job is claimable exactly once");
});

test("an expired job is never claimed, and a cancelled one is dropped", () => {
  const queue = tempRoot("expired");
  const jobs = join(queue, "jobs");
  mkdirSync(join(jobs, "expired-job"), { recursive: true });
  writeFileSync(join(jobs, "expired-job", "task.md"), "too late");
  writeFileSync(
    join(jobs, "expired-job", "meta.json"),
    JSON.stringify({ turn_id: "expired-job", workdir: "/tmp", created_at: 1, deadline_at: 2 }),
  );
  assert.equal(bridge(["claim"], { queue }), "", "a job past its deadline is not work");

  mkdirSync(join(jobs, "cancelled-job"), { recursive: true });
  writeFileSync(join(jobs, "cancelled-job", "task.md"), "stop");
  writeFileSync(
    join(jobs, "cancelled-job", "meta.json"),
    JSON.stringify({
      turn_id: "cancelled-job",
      workdir: "/tmp",
      created_at: 2,
      deadline_at: Date.now() / 1000 + 600,
    }),
  );
  writeFileSync(join(jobs, "cancelled-job", "cancel"), "client-cancelled\n");
  assert.equal(bridge(["claim"], { queue }), "", "a cancelled job is not claimed");
});

test("a done left by another turn is never reused as this turn's answer", async () => {
  const queue = tempRoot("stale");
  const workdir = workdirFor("stale-work");
  const acp = startBridge({ queue, env: { MAXPLAYER_MUSE_POLL_SECS: "0.05" } });
  await acp.call("initialize", {});
  const session = await acp.call("session/new", { cwd: workdir });
  const turn = acp.call("session/prompt", {
    sessionId: session.result.sessionId,
    ...promptBlocks("second task"),
  });
  const jobDir = await waitForJob(queue);
  // A worker run from an earlier turn writes its result into this job directory.
  writeFileSync(
    join(jobDir, "done"),
    JSON.stringify({ turn_id: "some-earlier-turn", status: "ok", summary: "old work" }),
  );
  const reply = await turn;
  assert.equal(reply.result.reason, "failed", "a mismatched turn_id must not complete the turn");
  acp.stop();
});

test("a turn that outlives its budget fails, and a late result cannot revive it", async () => {
  const queue = tempRoot("late");
  const workdir = workdirFor("late-work");
  const acp = startBridge({
    queue,
    env: {
      MAXPLAYER_MUSE_TURN_BUDGET_SECS: "1",
      MAXPLAYER_MUSE_POLL_SECS: "0.05",
      MAXPLAYER_MUSE_KEEPALIVE_SECS: "0.2",
    },
  });
  await acp.call("initialize", {});
  const session = await acp.call("session/new", { cwd: workdir });
  const turn = acp.call("session/prompt", {
    sessionId: session.result.sessionId,
    ...promptBlocks("slow task"),
  });
  const jobDir = await waitForJob(queue);
  const reply = await turn;
  assert.equal(reply.result.reason, "failed");

  // The expired turn told the worker to stop: the job is no longer claimable.
  assert.ok(existsSync(join(jobDir, "cancel")), "an expired turn cancels its own job");
  assert.equal(bridge(["claim"], { queue }), "", "a late worker run does not pick up a dead turn");

  // And a late done lands in the OLD job directory, so the next turn — a new
  // directory with a new turn id — cannot be completed by it.
  writeFileSync(
    join(jobDir, "done"),
    JSON.stringify({ turn_id: "late", status: "ok", summary: "too late" }),
  );
  const second = acp.call("session/prompt", {
    sessionId: session.result.sessionId,
    ...promptBlocks("next task"),
  });
  const secondReply = await second;
  assert.equal(secondReply.result.reason, "failed", "the next turn waits for its own result");
  acp.stop();
});

test("reap releases a dead run's claim and leaves everything else alone", () => {
  const queue = tempRoot("reap");
  const jobs = join(queue, "jobs");
  const live = join(jobs, "live-job");
  mkdirSync(join(live, "claim"), { recursive: true });
  writeFileSync(join(live, "task.md"), "work");
  writeFileSync(
    join(live, "meta.json"),
    JSON.stringify({ turn_id: "live-job", workdir: "/tmp", created_at: 1, deadline_at: Date.now() / 1000 + 600 }),
  );
  writeFileSync(join(live, "claim", "claim.json"), JSON.stringify({ claimed_at: 1, claim_token: "x" }));

  const fresh = join(jobs, "fresh-job");
  mkdirSync(join(fresh, "claim"), { recursive: true });
  writeFileSync(join(fresh, "task.md"), "work");
  writeFileSync(
    join(fresh, "meta.json"),
    JSON.stringify({ turn_id: "fresh-job", workdir: "/tmp", created_at: 2, deadline_at: Date.now() / 1000 + 600 }),
  );
  writeFileSync(
    join(fresh, "claim", "claim.json"),
    JSON.stringify({ claimed_at: Date.now() / 1000, claim_token: "y" }),
  );

  const released = bridge(["reap"], { queue }).split("\n").filter(Boolean);
  assert.deepEqual(released, [live], "only the abandoned claim is released");
  assert.equal(existsSync(join(fresh, "claim")), true, "a running worker keeps its claim");
  assert.equal(existsSync(join(live, "task.md")), true, "reap never destroys pending work");

  const reclaimed = JSON.parse(bridge(["claim"], { queue }));
  assert.equal(reclaimed.job_dir, live, "the released job is workable again after a restart");
});

// --- the published surface ---------------------------------------------------

const SKILLS_DIR = join(root, ".well-known", "skills");
const index = JSON.parse(readFileSync(join(SKILLS_DIR, "index.json"), "utf8"));
const MUSE_SKILLS = ["maxplayer-muse-buyer", "maxplayer-muse-seller"];

function frontmatter(text) {
  const match = /^---\n([\s\S]*?)\n---\n/.exec(text);
  assert.ok(match, "a skill must open with a YAML frontmatter block");
  const fields = {};
  for (const line of match[1].split("\n")) {
    const field = /^(\w+):\s*(.*)$/.exec(line);
    if (field) fields[field[1]] = field[2].trim();
  }
  return fields;
}

test("both Muse skills are published in the discovery index", () => {
  const names = index.skills.map(({ name }) => name);
  for (const name of MUSE_SKILLS) {
    assert.ok(names.includes(name), `${name} is missing from index.json`);
  }
});

test("every indexed skill resolves to a file whose frontmatter agrees with the index", () => {
  for (const skill of index.skills) {
    assert.match(skill.path, /^\/\.well-known\/skills\/[a-z-]+\/skill\.md$/);
    const file = join(root, skill.path.slice(1));
    assert.ok(existsSync(file), `${skill.name} points at a missing file: ${skill.path}`);
    const fields = frontmatter(readFileSync(file, "utf8"));
    // Muse finds a skill by searching names and frontmatter, so the frontmatter
    // name IS the installed identity: a mismatch publishes one skill under two
    // names and makes the description a trigger for something else.
    assert.equal(fields.name, skill.name, `${skill.path} frontmatter name`);
    assert.ok(fields.description && fields.description.length > 40,
      `${skill.name} needs a description that can trigger a search`);
    assert.ok(skill.description && skill.description.length > 40,
      `${skill.name} needs an index description`);
  }
});

test("every pointer the Muse skills publish resolves to a shipped file", () => {
  for (const name of MUSE_SKILLS) {
    const entry = index.skills.find((skill) => skill.name === name);
    const dir = dirname(join(root, entry.path.slice(1)));
    const pages = [join(dir, "skill.md")];
    const references = join(dir, "references");
    if (existsSync(references)) {
      for (const file of readdirSync(references)) pages.push(join(references, file));
    }
    let linked = 0;
    for (const page of pages) {
      const text = readFileSync(page, "utf8");
      const links = [...text.matchAll(/\]\((\/\.well-known\/[^)\s]+)\)/g)].map((m) => m[1]);
      linked += links.length;
      for (const link of links) {
        // A published skill's pointers are URLs on a live site. One that does not
        // resolve to a shipped file is a 404 the reader hits mid-procedure.
        assert.ok(existsSync(join(root, link.slice(1))),
          `${page} links ${link}, which is not shipped`);
      }
    }
    assert.ok(linked > 0, `${name} should point at its companions and references`);
  }
});

test("the Muse skills carry no operator identity, home path, key or balance", () => {
  // A public skill that ships one box's identity is not a public skill. These are
  // the exact shapes the source field reports were full of.
  const forbidden = [
    [/\/home\/[a-z]/i, "an absolute home path"],
    [/\/Users\/[a-z]/i, "an absolute home path"],
    [/\b[0-9a-f]{64}\b/, "a 64-hex key or pubkey"],
    [/\blnbc[0-9a-z]{20,}/i, "a Lightning invoice"],
    [/balance_sats\s*=\s*\d/, "a wallet balance"],
    [/\bdevice_id\b/, "a device id"],
  ];
  for (const name of MUSE_SKILLS) {
    const entry = index.skills.find((skill) => skill.name === name);
    const dir = dirname(join(root, entry.path.slice(1)));
    const files = [join(dir, "skill.md")];
    for (const sub of ["references", "bin"]) {
      const subdir = join(dir, sub);
      if (existsSync(subdir)) {
        for (const file of readdirSync(subdir)) files.push(join(subdir, file));
      }
    }
    for (const file of files) {
      const text = readFileSync(file, "utf8");
      for (const [pattern, what] of forbidden) {
        assert.equal(pattern.test(text), false, `${file} contains ${what}`);
      }
    }
  }
});

test("the seller skill ships the executable bridge it tells the reader to run", () => {
  assert.ok(existsSync(BRIDGE), "the bridge is published inside the skill directory");
  const skill = readFileSync(join(SKILLS_DIR, "muse-seller", "skill.md"), "utf8");
  assert.match(skill, /bin\/muse-acp-bridge\.py/);
  // Each subcommand the skill instructs a worker to run must exist.
  for (const command of ["selfcheck", "claim", "done", "reap"]) {
    assert.match(skill, new RegExp(`muse-acp-bridge\\.py[^\\n]*${command}|\`${command}\``),
      `the skill documents the ${command} subcommand`);
    assert.equal(
      spawnSync(PYTHON, [BRIDGE, command === "done" ? "--help" : command, "--help"].slice(0, 3), {
        encoding: "utf8",
        env: { ...process.env, MAXPLAYER_MUSE_QUEUE: tempRoot(`cmd-${command}`) },
      }).status,
      0,
      `${command} is a real subcommand`,
    );
  }
});

test("cancelling a live turn propagates to the worker and ends the turn as cancelled", async () => {
  const queue = tempRoot("cancel");
  const workdir = workdirFor("cancel-work");
  const acp = startBridge({ queue, env: { MAXPLAYER_MUSE_POLL_SECS: "0.05" } });
  await acp.call("initialize", {});
  const session = await acp.call("session/new", { cwd: workdir });
  const turn = acp.call("session/prompt", {
    sessionId: session.result.sessionId,
    ...promptBlocks("long task"),
  });
  const jobDir = await waitForJob(queue);
  await acp.call("session/cancel", { sessionId: session.result.sessionId });

  const reply = await turn;
  // "cancelled" is one of the three stop reasons the seller's driver recognises;
  // anything it does not recognise is read as failed.
  assert.equal(reply.result.reason, "cancelled");
  assert.ok(existsSync(join(jobDir, "cancel")), "the worker is told to stop, not left running");
  assert.equal(bridge(["claim"], { queue }), "", "a cancelled job is not handed to a worker");
  acp.stop();
});
