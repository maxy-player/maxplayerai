//! Prints the iptables plan [`NetPolicy`] would hand its sidecar, for gate evidence.
//!
//! This exists so the gVisor gate scripts install the rules the PRODUCT renders rather than rules a
//! script author transcribed by hand. A transcription drifts the moment the policy changes and the
//! gate keeps passing against a firewall the product no longer builds — which is the exact failure
//! the gates are supposed to catch.
//!
//! ```text
//! cargo run -p maxplayer-core --example render_net_plan -- 172.18.0.1 1.1.1.1
//! ```
//!
//! Arguments: the namespace gateway, then every resolver the job is allowed to reach on port 53.

use maxplayer_core::sandbox_net::{NetPolicy, PortRange};
use maxplayer_core::sandbox_netns::plan_stdin;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(gateway) = args.next() else {
        eprintln!("usage: render_net_plan <gateway> [resolver ...]");
        std::process::exit(2);
    };
    let dns_resolvers: Vec<String> = args.collect();
    let policy = NetPolicy {
        gateway,
        proxy_ports: Some(PortRange::new(49200, 49299).expect("a valid fixed range")),
        log_connections: true,
        dns_resolvers,
    };
    let (plan, count) = plan_stdin(&policy);
    eprintln!("# {count} rules");
    print!("{plan}");
}
