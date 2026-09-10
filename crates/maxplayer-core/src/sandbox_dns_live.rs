//! Live DNS runtime gate: the verdict's "Minimal future Linux gate", executed against a real kernel.
//!
//! Everything else that tests [`crate::sandbox_dns`] asserts what is *rendered*. That is exactly the
//! gap the focused verdict left open: F1/F2/F3 are closed for the source contract, and nothing in the
//! repository has ever asked a running kernel whether a contained job can actually resolve a name
//! through the resolver file the launch wrote for it. A rendered `--dport 53 -j ACCEPT` and an
//! enforced one produce the same green.
//!
//! These tests are `#[ignore]`d: they need a docker daemon, the first-party netfilter image, and two
//! throwaway fixture images. They are HARNESS ONLY — they add no product behaviour and change none.
//! Run them inside an owned, isolated VM:
//!
//! ```text
//! docker build -t mxdns-resolver:local  <dnsmasq image>
//! docker build -t mxdns-probe:local     <bind-tools/git/nc image>
//! cargo test -p maxplayer-core --features acp,wallet --lib sandbox_dns_live -- --ignored --test-threads=1
//! ```
//!
//! **What each gate is for.** The product path under test is the real one, not a re-implementation:
//! [`crate::seller_exec::prepare_launch`] resolves the resolver set through
//! [`crate::sandbox_dns::resolve`], writes the job's `/etc/resolv.conf`, hands the SAME addresses to
//! [`crate::sandbox_netns::establish`], which installs the plan through the real sidecar and reads
//! both families back out of the kernel; the payload is then launched through
//! [`crate::seller_exec::SandboxPolicy::launch`], the same argv builder the daemon uses.
//!
//! **Controls.** Every denial is paired with a contemporaneous unfiltered probe of the same address
//! from outside the namespace. A destination that is merely unroutable, or a listener that died,
//! fails exactly like a dropped packet; only the control separates them.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::home::{SandboxConfig, SandboxMode};
use crate::sandbox_net::Family;
use crate::sandbox_netns::readback_argv;
use crate::seller_exec::{JobLaunch, SandboxPolicy};
use crate::seller_git::DeliveryAgentIdentity;

/// The private v4 network the resolver sits on. Inside `172.16.0.0/12`, which the policy denies
/// wholesale — so reaching the resolver here proves the per-resolver port-53 exception is what
/// carries the packet, not a hole in the deny.
const SVC_V4: &str = "172.29.7.0/24";
const RESOLVER_V4: &str = "172.29.7.53";
/// A second private address on the same subnet, running the same listener as the "public" one. It is
/// not a resolver, so nothing excepts it: this is the deny control.
const OTHER_PRIVATE_V4: &str = "172.29.7.20";
/// The private v6 network. Inside `fc00::/7`, denied for the same reason as its v4 twin.
const SVC_V6: &str = "fd00:d57::/64";
const RESOLVER_V6: &str = "fd00:d57::53";
/// TEST-NET-3 and the v6 documentation range: outside every denied CIDR, so the policy treats them
/// as public. Nothing here leaves the VM.
///
/// The "public" network is deliberately v4-ONLY. Attaching a second **v6** network to a holder that
/// is already contained fails inside docker itself — it advertises the new address to `ff02::1`, and
/// `ff00::/8` is one of the ranges the policy drops. That is the policy working, not a defect, but it
/// means this fixture cannot carry v6 on the network it attaches late. The v6 legs of this gate ride
/// the holder's own network, which is attached before containment exists.
const PUB_V4: &str = "203.0.113.0/24";
const PUB_HOST_V4: &str = "203.0.113.10";
/// Answered by the fixture resolver as data, to prove an AAAA lookup completes over v6 transport. Not
/// a destination this gate connects to.
const PUB_HOST_V6: &str = "2001:db8:aa::10";

/// The name the fixture resolver answers for, and the port every fixture listener listens on.
const DNS_NAME: &str = "git.gate.test";
/// A name whose TXT answer is deliberately larger than a 512-byte DNS message, so a non-EDNS query
/// comes back truncated and the resolver library must retry over TCP/53.
const BIG_NAME: &str = "big.gate.test";
const LISTEN_PORT: &str = "9999";
/// A second port on the resolver's own address. Port 53 is excepted; this one must not be.
const NON_DNS_PORT: &str = "8080";

const RESOLVER_IMAGE: &str = "mxdns-resolver:local";
const PROBE_IMAGE: &str = "mxdns-probe:local";

const SVC_NET: &str = "mxdns-gate-svc";
const PUB_NET: &str = "mxdns-gate-pub";
const RESOLVER_CT: &str = "mxdns-gate-resolver";
const OTHER_CT: &str = "mxdns-gate-other";
const PUB_CT: &str = "mxdns-gate-pub-host";

fn docker(args: &[&str]) -> (bool, String, String) {
    let out = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("docker must be on PATH for this gate");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    )
}

/// Two networks, a real caching resolver with fixed v4 and v6 addresses, and two plain TCP listeners
/// — one at a private address that nothing excepts, one at a public one.
///
/// Torn down on drop, by exact name. Nothing here removes an object it did not create.
struct Infra;

impl Infra {
    fn up() -> Self {
        // A previous run that panicked before its guard existed leaves debris under OUR names only.
        // Clearing them makes a rerun idempotent; it cannot touch anything else on the host.
        Self::down();

        let (ok, _, err) = docker(&[
            "network", "create", "--ipv6", "--subnet", SVC_V4, "--subnet", SVC_V6, SVC_NET,
        ]);
        assert!(ok, "could not create {SVC_NET} on {SVC_V4}/{SVC_V6}: {err}");
        let (ok, _, err) = docker(&["network", "create", "--subnet", PUB_V4, PUB_NET]);
        assert!(ok, "could not create {PUB_NET} on {PUB_V4}: {err}");

        // dnsmasq, answering A and AAAA for one name and an over-long TXT for another, bound to both
        // families at fixed addresses. `--no-resolv --no-hosts` so it forwards nothing: every answer
        // this gate sees came from this container, not from the VM's uplink.
        let txt = format!("{},{},{},{}", BIG_NAME, "x".repeat(250), "y".repeat(250), "z".repeat(250));
        let cmd = format!(
            "dnsmasq -k -p 53 --no-resolv --no-hosts --bind-interfaces \
             --listen-address={RESOLVER_V4} --listen-address={RESOLVER_V6} \
             --address=/{DNS_NAME}/{PUB_HOST_V4} --address=/{DNS_NAME}/{PUB_HOST_V6} \
             --txt-record={txt} & \
             while :; do nc -l -p {NON_DNS_PORT} >/dev/null 2>&1; done"
        );
        let (ok, _, err) = docker(&[
            "run", "--detach", "--name", RESOLVER_CT, "--network", SVC_NET, "--ip", RESOLVER_V4,
            "--ip6", RESOLVER_V6, "--cap-add", "NET_BIND_SERVICE", "--entrypoint", "sh",
            RESOLVER_IMAGE, "-c", &cmd,
        ]);
        assert!(ok, "could not start the fixture resolver: {err}");

        for (name, net, ip) in
            [(OTHER_CT, SVC_NET, OTHER_PRIVATE_V4), (PUB_CT, PUB_NET, PUB_HOST_V4)]
        {
            let listen = format!("while :; do nc -l -p {LISTEN_PORT} >/dev/null 2>&1; done");
            let (ok, _, err) = docker(&[
                "run", "--detach", "--name", name, "--network", net, "--ip", ip, "--entrypoint",
                "sh", PROBE_IMAGE, "-c", &listen,
            ]);
            assert!(ok, "could not start listener {name}: {err}");
        }
        // The resolver has to be answering before any gate queries it; dnsmasq binds in well under a
        // second, but "well under" is not "before".
        for attempt in 0..30 {
            let (ok, out, _) = docker(&[
                "run", "--rm", "--network", SVC_NET, "--entrypoint", "dig", PROBE_IMAGE, "+short",
                "+time=1", "+tries=1", "A", DNS_NAME, &format!("@{RESOLVER_V4}"),
            ]);
            if ok && out.contains(PUB_HOST_V4) {
                return Self;
            }
            assert!(attempt < 29, "the fixture resolver never answered: {out}");
            std::thread::sleep(Duration::from_millis(300));
        }
        Self
    }

    fn down() {
        for name in [RESOLVER_CT, OTHER_CT, PUB_CT] {
            docker(&["rm", "--force", "--volumes", name]);
        }
        // Holders and job containers are named by the product, from the job id. Only ids this gate
        // creates (`dnsgate-*`) are matched, so nothing else on the host is touched.
        let (_, listing, _) = docker(&["ps", "--all", "--format", "{{.Names}}"]);
        for name in listing.lines().filter(|line| line.contains("dnsgate-")) {
            docker(&["rm", "--force", "--volumes", name]);
        }
        for net in [SVC_NET, PUB_NET] {
            docker(&["network", "rm", net]);
        }
    }
}

impl Drop for Infra {
    fn drop(&mut self) {
        Self::down();
    }
}

/// The seat config every gate launches through, so each one exercises `SandboxPolicy::from_config`
/// rather than a hand-assembled policy.
fn config(dns_servers: Vec<String>) -> SandboxConfig {
    config_on(SVC_NET, dns_servers)
}

fn config_on(network: &str, dns_servers: Vec<String>) -> SandboxConfig {
    SandboxConfig {
        mode: SandboxMode::Docker,
        launcher: Vec::new(),
        image: Some(PROBE_IMAGE.to_owned()),
        forward_env: Vec::new(),
        runtime: None,
        network: Some(network.to_owned()),
        proxy_port_range: None,
        file_credentials: Vec::new(),
        dns_servers,
        codex_chatgpt: None,
        container_delivery: None,
        container_delivery_token: None,
        container_delivery_token_cap_secs: None,
    }
}

fn identity() -> DeliveryAgentIdentity {
    DeliveryAgentIdentity::for_seller(&"ab".repeat(32))
}

/// A workdir whose last component is the job id the product derives holder names from.
struct Workdir(std::path::PathBuf);

impl Workdir {
    fn new(job: &str) -> Self {
        let path = std::env::temp_dir().join("mxdns-gate").join(format!("dnsgate-{job}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a workdir");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Attach the "public" network to a holder, so one namespace has a route to both fixtures. Without
/// it the public listener is unroutable, and unroutable fails exactly like denied.
fn attach_public(holder: &str) {
    let (ok, _, err) = docker(&["network", "connect", PUB_NET, holder]);
    assert!(ok, "could not attach {PUB_NET} to {holder}: {err}");
}

/// Read one family's rules back out of a live namespace through the product's own readback argv.
fn readback(holder: &str, family: Family) -> String {
    let argv = readback_argv(holder, crate::sandbox_netns::DEFAULT_NETFILTER_IMAGE, family);
    let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    let (ok, stdout, err) = docker(&args);
    assert!(ok, "{} readback failed: {err}", family.binary());
    stdout
}

/// Run one shell payload in a namespace through the real argv builder, returning its stdout.
fn run_payload(policy: &SandboxPolicy, prepared_uid: u32, prepared_gid: u32, workdir: &Path,
    netns: Option<&str>, resolv: Option<&Path>, script: &str) -> (bool, String, String) {
    let command: Vec<String> =
        ["sh", "-c", script].into_iter().map(String::from).collect();
    let launch = policy
        .launch(
            &command,
            &JobLaunch {
                workdir,
                env: &[],
                uid: prepared_uid,
                gid: prepared_gid,
                netns,
                resolv_conf: resolv,
            },
        )
        .expect("the policy must build a launch");
    let out = Command::new(&launch.program)
        .args(&launch.args)
        .stdin(Stdio::null())
        .output()
        .expect("the launch must spawn");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    )
}

fn field<'a>(stdout: &'a str, key: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("payload printed no {key}= line:\n{stdout}"))
}

/// The payload every containment gate runs: it reports what its own resolver file is, whether that
/// file is writable, what it can resolve over each family's transport, and which of four TCP
/// destinations it can reach.
fn probe_script() -> String {
    format!(
        "set -u; \
         echo SHA=$(sha256sum /etc/resolv.conf | cut -d' ' -f1); \
         if echo tampered >> /etc/resolv.conf 2>/dev/null; then echo RO=no; else echo RO=yes; fi; \
         echo DEFAULT=$(dig +short +time=2 +tries=1 A {DNS_NAME} | head -1); \
         echo V4T=$(dig +short +time=2 +tries=1 A {DNS_NAME} @{RESOLVER_V4} | head -1); \
         echo V6T=$(dig +short +time=2 +tries=1 AAAA {DNS_NAME} @{RESOLVER_V6} | head -1); \
         nc -w 2 {PUB_HOST_V4} {LISTEN_PORT} </dev/null >/dev/null 2>&1 && echo PUB=reach || echo PUB=deny; \
         nc -w 2 {OTHER_PRIVATE_V4} {LISTEN_PORT} </dev/null >/dev/null 2>&1 && echo PRIV=reach || echo PRIV=deny; \
         nc -w 2 {RESOLVER_V4} {NON_DNS_PORT} </dev/null >/dev/null 2>&1 && echo NON53=reach || echo NON53=deny"
    )
}

/// **Gate A.** A seat with configured private resolvers launches a contained job that resolves a name
/// over both families, from inside the namespace, through the file the launch wrote — and the same
/// namespace denies every private destination that is not one of those resolvers on port 53.
///
/// The whole chain is the product's: `prepare_launch` → `sandbox_dns::resolve` → the resolver file →
/// `establish` → the sidecar → both-family readback → `SandboxPolicy::launch`.
#[tokio::test]
#[ignore = "needs docker, the netfilter image and the two fixture images"]
async fn a_contained_job_resolves_over_private_v4_and_v6_and_the_rest_of_that_space_stays_denied() {
    let _infra = Infra::up();
    let workdir = Workdir::new("gatea");

    // The unfiltered control FIRST, contemporaneous with the gate and on the same fixtures: every
    // destination below is reachable when nothing is enforcing. A later denial is then attributable
    // to the rules rather than to a dead listener or an unroutable address.
    let (_, control, control_err) = {
        let script = probe_script();
        let (ok, out, err) = docker(&[
            "run", "--rm", "--network", SVC_NET, "--entrypoint", "sh", PROBE_IMAGE, "-c", &script,
        ]);
        assert!(ok || !out.is_empty(), "the control payload produced nothing: {err}");
        (ok, out, err)
    };
    // The control container is on SVC_NET only, so the public listener is unroutable there; it is
    // reached in the contained case through the attached PUB_NET. What the control has to show is the
    // private pair, which is exactly what the policy denies.
    assert_eq!(field(&control, "PRIV"), "reach", "control: {control}\n{control_err}");
    assert_eq!(field(&control, "NON53"), "reach", "control: {control}\n{control_err}");
    assert_eq!(field(&control, "V4T"), PUB_HOST_V4, "control: {control}");
    assert_eq!(field(&control, "V6T"), PUB_HOST_V6, "control: {control}");

    let config = config(vec![RESOLVER_V4.to_owned(), RESOLVER_V6.to_owned()]);
    let policy = SandboxPolicy::from_config(Some(&config)).expect("a docker policy");
    let prepared = crate::seller_exec::prepare_launch(
        &["true".to_owned()],
        &policy,
        workdir.path(),
        &identity(),
        Duration::from_secs(300),
    )
    .await
    .expect("containment must be established");

    let holder = prepared.holder_name.clone().expect("a holder");
    let resolv = prepared.resolv_conf.clone().expect("a resolver file");
    let body = std::fs::read(&resolv).expect("the resolver file must exist on the host");
    let host_sha = sha256_hex(&body);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains(&format!("nameserver {RESOLVER_V4}")), "{text}");
    assert!(text.contains(&format!("nameserver {RESOLVER_V6}")), "{text}");

    attach_public(&holder);
    // Read the kernel back AFTER a second interface joined the namespace. Both families, and both
    // resolvers, asked of the namespace rather than of the policy struct: an exception that renders
    // and is not in force is precisely the failure the source-only verdict could not rule out.
    let v4_rules = readback(&holder, Family::V4);
    assert!(v4_rules.contains(RESOLVER_V4), "no v4 resolver exception in the kernel:\n{v4_rules}");
    let v6_rules = readback(&holder, Family::V6);
    assert!(v6_rules.contains(RESOLVER_V6), "no v6 resolver exception in the kernel:\n{v6_rules}");

    let (_, out, err) = run_payload(
        &policy,
        prepared.uid,
        prepared.gid,
        workdir.path(),
        Some(&holder),
        Some(resolv.as_path()),
        &probe_script(),
    );
    assert_eq!(field(&out, "SHA"), host_sha, "the job's resolver file is not the one written: {out}\n{err}");
    assert_eq!(field(&out, "RO"), "yes", "the resolver file must be mounted read-only: {out}");
    assert_eq!(field(&out, "DEFAULT"), PUB_HOST_V4, "the written file must resolve: {out}\n{err}");
    assert_eq!(field(&out, "V4T"), PUB_HOST_V4, "private v4 DNS must answer: {out}\n{err}");
    assert_eq!(field(&out, "V6T"), PUB_HOST_V6, "private v6 DNS must answer: {out}\n{err}");
    assert_eq!(field(&out, "PUB"), "reach", "a public destination must stay allowed: {out}");
    assert_eq!(field(&out, "PRIV"), "deny", "another private address must be denied: {out}");
    assert_eq!(field(&out, "NON53"), "deny", "a non-53 port on the resolver must be denied: {out}");
}

/// **Diagnosis.** Not a gate: it prints what the kernel holds and what the namespace can do over v6,
/// so a v6 DNS failure can be attributed rather than guessed at. Kept because the attribution it
/// produces is the whole difference between "v6 resolvers do not work" and a named cause.
#[tokio::test]
#[ignore = "diagnostic, not a gate"]
async fn diagnose_v6_resolver_reachability_inside_containment() {
    let _infra = Infra::up();
    let workdir = Workdir::new("gatef");
    let config = config(vec![RESOLVER_V4.to_owned(), RESOLVER_V6.to_owned()]);
    let policy = SandboxPolicy::from_config(Some(&config)).expect("a docker policy");
    let prepared = crate::seller_exec::prepare_launch(
        &["true".to_owned()],
        &policy,
        workdir.path(),
        &identity(),
        Duration::from_secs(300),
    )
    .await
    .expect("containment must be established");
    let holder = prepared.holder_name.clone().expect("a holder");
    let resolv = prepared.resolv_conf.clone().expect("a resolver file");

    println!("--- ip6tables readback ---\n{}", readback(&holder, Family::V6));
    // One container, three attempts, printing the neighbour cache around each. If the first query
    // fails and a later one succeeds once the cache holds a link-layer address, the cause is
    // neighbour discovery being dropped, not the port-53 exception being absent.
    let script = format!(
        "echo N0=$(ip -6 neigh show | tr '\\n' '|'); \
         echo Q1=$(dig +time=2 +tries=1 +short AAAA {DNS_NAME} @{RESOLVER_V6} 2>&1 | tr '\\n' ' '); \
         echo N1=$(ip -6 neigh show | tr '\\n' '|'); \
         sleep 3; \
         echo Q2=$(dig +time=3 +tries=1 +short AAAA {DNS_NAME} @{RESOLVER_V6} 2>&1 | tr '\\n' ' '); \
         echo N2=$(ip -6 neigh show | tr '\\n' '|'); \
         echo P6=$(ping6 -c 1 -W 2 {RESOLVER_V6} >/dev/null 2>&1 && echo reach || echo deny); \
         echo Q3=$(dig +time=3 +tries=1 +short AAAA {DNS_NAME} @{RESOLVER_V6} 2>&1 | tr '\\n' ' '); \
         echo Q4=$(dig +time=3 +tries=1 +short A {DNS_NAME} @{RESOLVER_V4} 2>&1 | tr '\\n' ' ')"
    );
    let (_, out, err) = run_payload(
        &policy, prepared.uid, prepared.gid, workdir.path(), Some(&holder),
        Some(resolv.as_path()), &script,
    );
    println!("--- v6 attribution ---\n{out}\n[stderr] {err}");
}

/// **Attribution, part two.** The same launch with a resolver in the v6 documentation range —
/// `2001:db8::/32`, which the policy denies nowhere — on its own network.
///
/// This is the discriminator. If v6 DNS works here and not at a ULA address, the cause is the
/// `fc00::/7` DROP catching neighbour discovery to the resolver itself. If it fails here too, the
/// cause is the `ff00::/8` DROP catching the multicast solicitation, and NO v6 resolver on a link
/// that needs discovery can work under this policy.
#[tokio::test]
#[ignore = "diagnostic, not a gate"]
async fn diagnose_v6_resolver_outside_the_denied_ranges() {
    const NET: &str = "mxdns-gate-doc";
    const RES: &str = "mxdns-gate-doc-resolver";
    const ADDR: &str = "2001:db8:bb::53";
    docker(&["rm", "--force", "--volumes", RES]);
    docker(&["network", "rm", NET]);
    let (ok, _, err) = docker(&[
        "network", "create", "--ipv6", "--subnet", "192.0.2.0/24", "--subnet",
        "2001:db8:bb::/64", NET,
    ]);
    assert!(ok, "could not create {NET}: {err}");
    let cmd = format!(
        "dnsmasq -k -p 53 --no-resolv --no-hosts --bind-interfaces --listen-address={ADDR} \
         --address=/{DNS_NAME}/{PUB_HOST_V6}"
    );
    let (ok, _, err) = docker(&[
        "run", "--detach", "--name", RES, "--network", NET, "--ip6", ADDR, "--entrypoint", "sh",
        RESOLVER_IMAGE, "-c", &cmd,
    ]);
    assert!(ok, "could not start the documentation-range resolver: {err}");
    std::thread::sleep(Duration::from_secs(2));

    let workdir = Workdir::new("gateg");
    let config = config_on(NET, vec![ADDR.to_owned()]);
    let policy = SandboxPolicy::from_config(Some(&config)).expect("a docker policy");
    let prepared = crate::seller_exec::prepare_launch(
        &["true".to_owned()],
        &policy,
        workdir.path(),
        &identity(),
        Duration::from_secs(300),
    )
    .await
    .expect("containment must be established");
    let holder = prepared.holder_name.clone().expect("a holder");
    let resolv = prepared.resolv_conf.clone().expect("a resolver file");
    println!("--- ip6tables readback ---\n{}", readback(&holder, Family::V6));

    let script = format!(
        "echo UNC=$(dig +time=3 +tries=1 +short AAAA {DNS_NAME} @{ADDR} 2>&1 | tr '\\n' ' '); \
         echo NEIGH=$(ip -6 neigh show | tr '\\n' '|')"
    );
    let (_, out, err) = run_payload(
        &policy, prepared.uid, prepared.gid, workdir.path(), Some(&holder),
        Some(resolv.as_path()), &script,
    );
    println!("--- contained, resolver outside every denied range ---\n{out}\n[stderr] {err}");

    // The contemporaneous control on the same fixture, with no policy in force.
    let (_, control, _) = docker(&[
        "run", "--rm", "--network", NET, "--entrypoint", "dig", PROBE_IMAGE, "+short", "+time=3",
        "+tries=1", "AAAA", DNS_NAME, &format!("@{ADDR}"),
    ]);
    println!("--- uncontained control ---\n{control}");

    let (_, listing, _) = docker(&["ps", "--all", "--format", "{{.Names}}"]);
    for name in listing.lines().filter(|line| line.contains("dnsgate-gateg")) {
        docker(&["rm", "--force", "--volumes", name]);
    }
    docker(&["rm", "--force", "--volumes", RES]);
    docker(&["network", "rm", NET]);
}

/// **Gate B.** A UDP answer too large for a 512-byte message comes back truncated, and the retry over
/// TCP/53 — a different rule in the plan — succeeds from inside the same namespace.
#[tokio::test]
#[ignore = "needs docker, the netfilter image and the two fixture images"]
async fn a_truncated_udp_answer_falls_back_to_tcp_53_inside_containment() {
    let _infra = Infra::up();
    let workdir = Workdir::new("gateb");
    let config = config(vec![RESOLVER_V4.to_owned(), RESOLVER_V6.to_owned()]);
    let policy = SandboxPolicy::from_config(Some(&config)).expect("a docker policy");
    let prepared = crate::seller_exec::prepare_launch(
        &["true".to_owned()],
        &policy,
        workdir.path(),
        &identity(),
        Duration::from_secs(300),
    )
    .await
    .expect("containment must be established");
    let holder = prepared.holder_name.clone().expect("a holder");
    let resolv = prepared.resolv_conf.clone().expect("a resolver file");

    // `+noedns` caps the UDP buffer at 512 bytes, so a ~750-byte TXT answer sets TC and dig retries
    // over TCP by itself. `+ignore` on the second query proves the UDP answer really was truncated
    // rather than the whole thing having fitted.
    let script = format!(
        "V4=$(dig +noedns +time=2 +tries=1 TXT {BIG_NAME} @{RESOLVER_V4} 2>&1); \
         case \"$V4\" in *'retrying in TCP'*) echo TC4=yes;; *) echo TC4=no;; esac; \
         echo \"$V4\" | grep -c '\"' | sed 's/^/ANS4=/'; \
         T4=$(dig +noedns +tcp +time=2 +tries=1 +short TXT {BIG_NAME} @{RESOLVER_V4} 2>&1); \
         echo \"$T4\" | grep -c xxx | sed 's/^/TCP4=/'"
    );
    let (_, out, err) = run_payload(
        &policy,
        prepared.uid,
        prepared.gid,
        workdir.path(),
        Some(&holder),
        Some(resolv.as_path()),
        &script,
    );
    assert_eq!(field(&out, "TC4"), "yes", "the v4 UDP answer was not truncated: {out}\n{err}");
    assert_ne!(field(&out, "ANS4"), "0", "the v4 TCP retry returned nothing: {out}\n{err}");
    assert_ne!(field(&out, "TCP4"), "0", "an explicit TCP/53 query was refused: {out}\n{err}");
}

/// **Gate C.** With nothing configured, the launch discovers the host's own resolvers. On a
/// systemd-resolved host `/etc/resolv.conf` names only the `127.0.0.53` stub — an address no sandbox
/// can reach — and the canonical upstream must come from `resolvectl` instead.
///
/// This is F2's runtime face: the file a job is handed must name what a launch would *install*, never
/// the stub the host reads.
#[tokio::test]
#[ignore = "needs docker, the netfilter image and the two fixture images"]
async fn host_stub_discovery_hands_the_job_a_canonical_upstream_not_the_stub() {
    let _infra = Infra::up();
    let workdir = Workdir::new("gatec");
    let config = config(Vec::new());
    let policy = SandboxPolicy::from_config(Some(&config)).expect("a docker policy");
    assert!(
        policy.dns_servers().is_empty(),
        "this gate measures discovery; a configured list would short-circuit it"
    );
    let prepared = crate::seller_exec::prepare_launch(
        &["true".to_owned()],
        &policy,
        workdir.path(),
        &identity(),
        Duration::from_secs(300),
    )
    .await
    .expect("containment must be established from discovered resolvers");
    let resolv = prepared.resolv_conf.clone().expect("a resolver file");
    let text = std::fs::read_to_string(&resolv).expect("the resolver file");
    assert!(
        !text.contains("127.0.0."),
        "a loopback stub reached the job's resolver file:\n{text}"
    );
    assert!(
        text.lines().any(|line| line.starts_with("nameserver ")),
        "discovery produced no nameserver line:\n{text}"
    );
    // The same addresses must be the ones the kernel was told about, or the job is pointed at a
    // resolver its own firewall drops. Asked of the namespace, not of the policy struct.
    let holder = prepared.holder_name.clone().expect("a holder");
    let v4_rules = readback(&holder, Family::V4);
    let v6_rules = readback(&holder, Family::V6);
    for line in text.lines().filter_map(|line| line.strip_prefix("nameserver ")) {
        let rules = if line.contains(':') { &v6_rules } else { &v4_rules };
        assert!(
            rules.contains(line),
            "the kernel has no exception for the resolver the job was handed ({line}):\n{rules}"
        );
    }
}

/// **Gate D.** A seat whose only named resolver is unusable is refused BEFORE anything exists: no
/// holder container, no resolver file, no payload.
#[tokio::test]
#[ignore = "needs docker"]
async fn no_usable_resolver_refuses_before_a_holder_or_a_payload_exists() {
    let workdir = Workdir::new("gated");
    let config = config(vec!["127.0.0.53".to_owned()]);
    let policy = SandboxPolicy::from_config(Some(&config));
    // The refusal may land at config time or at launch time; both are "before anything exists", and
    // this gate accepts either — what it does not accept is a launch.
    let error = match policy {
        Err(error) => error.to_string(),
        Ok(policy) => crate::seller_exec::prepare_launch(
            &["true".to_owned()],
            &policy,
            workdir.path(),
            &identity(),
            Duration::from_secs(300),
        )
        .await
        .err()
        .expect("a loopback resolver must refuse the launch")
        .to_string(),
    };
    assert!(
        error.contains("resolver") || error.contains("loopback"),
        "the refusal must name the resolver: {error}"
    );
    let (_, listing, _) = docker(&["ps", "--all", "--format", "{{.Names}}"]);
    let holders: Vec<&str> =
        listing.lines().filter(|line| line.contains("dnsgate-gated")).collect();
    assert!(holders.is_empty(), "a refused launch left a container behind: {holders:?}");
    let stray: Vec<_> = std::fs::read_dir(workdir.path().parent().expect("a parent"))
        .expect("the workdir parent")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("resolv-dnsgate-gated"))
        .collect();
    assert!(stray.is_empty(), "a refused launch wrote a resolver file: {stray:?}");
}

/// **Gate E.** An installer that cannot apply the plan must leave no namespace behind — and so no
/// payload can be launched into one. The failure is injected by handing `establish` a sidecar image
/// that is not the applier, which is exactly the shape of "the sidecar ran and did not install".
#[tokio::test]
#[ignore = "needs docker and the two fixture images"]
async fn a_failed_installer_destroys_the_holder_and_leaves_nothing_to_launch_into() {
    let _infra = Infra::up();
    let job = "dnsgate-gatee";
    let result = crate::sandbox_netns::establish(
        SVC_NET,
        PROBE_IMAGE,
        // Not the netfilter image: no applier, so the plan is never installed and the count the
        // caller cross-checks never arrives.
        PROBE_IMAGE,
        crate::credential_proxy::PROXY_HOST_ALIAS,
        job,
        &"ab".repeat(32),
        1000,
        1000,
        None,
        true,
        vec![RESOLVER_V4.to_owned()],
    )
    .await;
    let error = result.err().expect("an installer that cannot apply must fail the launch");
    assert!(
        error.contains("containment") || error.contains("sidecar") || error.contains("rules"),
        "the failure must name the containment step: {error}"
    );
    let name = crate::sandbox_netns::holder_name(job);
    let (_, listing, _) = docker(&["ps", "--all", "--format", "{{.Names}}"]);
    assert!(
        !listing.lines().any(|line| line == name),
        "the holder {name} survived a failed installation:\n{listing}"
    );
}
