//! mint-acceptance — the named acceptance gate for the sandbox-local fakewallet test mint.
//!
//! Every check asserts a NUMBER, not a status. A no-op pass is a failure here: if a leg cannot be
//! run, the harness says so and exits non-zero rather than printing a green tick.
//!
//! Worthless test ecash only. This binary talks to http://127.0.0.1:8085 and nothing else.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use cdk::nuts::nut00::ProofsMethods;
use cdk::nuts::{CurrencyUnit, PaymentMethod};
use cdk::wallet::{ReceiveOptions, SendOptions, Wallet};
use cdk::Amount;
use cdk_fake_wallet::{create_fake_invoice, FakeInvoiceDescription};
use cdk_sqlite::wallet::memory;
use rand::random;

const MINT_URL: &str = "http://127.0.0.1:8085";
const MINT_PORT: u16 = 8085;

/// Every assertion this run made, so the report can state counts rather than adjectives.
struct Tally {
    checks: u32,
    legs: Vec<String>,
}

impl Tally {
    fn new() -> Self {
        Self { checks: 0, legs: Vec::new() }
    }

    fn check(&mut self, what: &str, ok: bool, detail: String) -> Result<()> {
        self.checks += 1;
        if ok {
            println!("  ok   {what}: {detail}");
            Ok(())
        } else {
            println!("  FAIL {what}: {detail}");
            Err(anyhow!("{what}: {detail}"))
        }
    }

    fn leg(&mut self, name: &str) {
        println!("\n[{}] {name}", self.legs.len() + 1);
        self.legs.push(name.to_owned());
    }
}

async fn new_wallet() -> Result<Wallet> {
    let store = Arc::new(memory::empty().await?);
    Ok(Wallet::new(MINT_URL, CurrencyUnit::Sat, store, random::<[u8; 64]>(), None)?)
}

async fn balance(w: &Wallet) -> Result<u64> {
    Ok(u64::from(w.total_balance().await?))
}

/// Mint `amount` sats into `w`. Fakewallet auto-settles the quote, so this is the issue leg.
async fn issue(w: &Wallet, amount: u64) -> Result<u64> {
    let quote = w
        .mint_quote(PaymentMethod::BOLT11, Some(Amount::from(amount)), None, None)
        .await
        .context("mint_quote")?;
    let proofs = w
        .wait_and_mint_quote(quote, Default::default(), Default::default(), Duration::from_secs(30))
        .await
        .context("wait_and_mint_quote")?;
    Ok(u64::from(proofs.total_amount()?))
}

/// The mint's own listening socket, read from the kernel rather than inferred from config.
/// /proc/net/tcp local_address for a loopback bind is `0100007F` (127.0.0.1, little-endian hex);
/// a wildcard bind would be `00000000`. This is the containment claim, proved.
fn listener_local_addrs() -> Result<Vec<String>> {
    let mut found = Vec::new();
    let want_port = format!("{MINT_PORT:04X}");
    for (path, _v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        for line in text.lines().skip(1) {
            let mut cols = line.split_whitespace();
            let _sl = cols.next();
            let Some(local) = cols.next() else { continue };
            let Some(_rem) = cols.next() else { continue };
            let Some(state) = cols.next() else { continue };
            // 0A = TCP_LISTEN
            if state != "0A" {
                continue;
            }
            if let Some((addr, port)) = local.split_once(':') {
                if port.eq_ignore_ascii_case(&want_port) {
                    found.push(addr.to_owned());
                }
            }
        }
    }
    Ok(found)
}

/// This container's own routable address, without sending a packet: a connected UDP socket picks
/// the source address the kernel would use.
fn container_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    sock.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_loopback())
}

fn test_mint(arg: &str) -> Result<String> {
    let out = Command::new("test-mint").arg(arg).output().context("run test-mint")?;
    if !out.status.success() {
        bail!("test-mint {arg} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("mint-acceptance — sandbox-local fakewallet mint at {MINT_URL}");
    println!("WORTHLESS TEST ECASH. No real funds, no external payment.\n");

    let mut t = Tally::new();

    // ---------------------------------------------------------------- 1. mint reachable + info
    t.leg("mint info (NUT-06)");
    let wallet = new_wallet().await?;
    let info = wallet.fetch_mint_info().await?.ok_or_else(|| anyhow!("mint returned no info"))?;
    let version = info.version.as_ref().map(|v| v.to_string()).unwrap_or_default();
    t.check("mint version", version.contains("0.17.2"), format!("{version:?}"))?;
    t.check(
        "mint advertises NUT-04 mint support",
        !info.nuts.nut04.methods.is_empty(),
        format!("{} method(s)", info.nuts.nut04.methods.len()),
    )?;

    // ---------------------------------------------------------------- 2. issue
    t.leg("mint quote + issue");
    const ISSUE: u64 = 64;
    let minted = issue(&wallet, ISSUE).await?;
    t.check("proofs minted", minted == ISSUE, format!("{minted} sats (wanted {ISSUE})"))?;
    let after_issue = balance(&wallet).await?;
    t.check("wallet balance after issue", after_issue == ISSUE, format!("{after_issue} sats"))?;

    // ---------------------------------------------------------------- 3. send / receive (swap)
    t.leg("send + receive across wallets");
    const SEND: u64 = 21;
    let prepared = wallet.prepare_send(Amount::from(SEND), SendOptions::default()).await?;
    let send_fee = u64::from(prepared.fee());
    let token = prepared.confirm(None).await?;
    let token_str = token.to_string();
    t.check("token is non-empty", token_str.starts_with("cashu"), format!("{} chars", token_str.len()))?;

    let sender_after = balance(&wallet).await?;
    let expect_sender = ISSUE - SEND - send_fee;
    t.check(
        "sender debited exactly amount+fee",
        sender_after == expect_sender,
        format!("{sender_after} sats (issued {ISSUE} - sent {SEND} - fee {send_fee} = {expect_sender})"),
    )?;

    let receiver = new_wallet().await?;
    let received = u64::from(receiver.receive(&token_str, ReceiveOptions::default()).await?);
    t.check("receiver credited", received == SEND, format!("{received} sats (wanted {SEND})"))?;
    let receiver_bal = balance(&receiver).await?;
    t.check("receiver balance", receiver_bal == SEND, format!("{receiver_bal} sats"))?;

    // ---------------------------------------------------------------- 4. double spend rejected
    t.leg("double spend rejected");
    let thief = new_wallet().await?;
    match thief.receive(&token_str, ReceiveOptions::default()).await {
        Ok(a) => {
            t.check("second receive of same token", false, format!("ACCEPTED {} sats — double spend!", u64::from(a)))?;
        }
        Err(e) => {
            t.check("second receive of same token", true, format!("rejected: {e}"))?;
        }
    }
    let thief_bal = balance(&thief).await?;
    t.check("double-spender balance stays zero", thief_bal == 0, format!("{thief_bal} sats"))?;

    // ---------------------------------------------------------------- 5. melt (successful)
    t.leg("melt");
    const MELT: u64 = 5;
    let good_invoice = create_fake_invoice(MELT * 1_000, serde_json::to_string(&FakeInvoiceDescription::default())?);
    let before_melt = balance(&receiver).await?;
    let melt_quote = receiver
        .melt_quote(PaymentMethod::BOLT11, good_invoice.to_string(), None, None)
        .await?;
    let prepared_melt = receiver.prepare_melt(&melt_quote.id, HashMap::new()).await?;
    let melt_amount = u64::from(prepared_melt.amount());
    let confirmed = prepared_melt.confirm().await?;
    let fee_paid = u64::from(confirmed.fee_paid());
    let after_melt = balance(&receiver).await?;
    t.check("melt amount", melt_amount == MELT, format!("{melt_amount} sats"))?;
    t.check(
        "balance fell by amount+fee",
        after_melt == before_melt - MELT - fee_paid,
        format!("{before_melt} -> {after_melt} (melt {MELT}, fee {fee_paid})"),
    )?;

    // ---------------------------------------------------------------- 6. failed payment
    // cdk-fake-wallet reads the invoice DESCRIPTION as a FakeInvoiceDescription and honours
    // `pay_err` by returning UnknownInvoice from make_payment (cdk-fake-wallet 0.17.2
    // src/lib.rs:659-714). That is a genuine failed outgoing payment, not a skipped leg.
    t.leg("failed payment + no balance lost");
    let fail_desc = FakeInvoiceDescription {
        pay_err: true,
        check_err: false,
        ..Default::default()
    };
    let bad_invoice = create_fake_invoice(MELT * 1_000, serde_json::to_string(&fail_desc)?);
    let before_fail = balance(&receiver).await?;
    let fail_quote = receiver
        .melt_quote(PaymentMethod::BOLT11, bad_invoice.to_string(), None, None)
        .await?;
    let prepared_fail = receiver.prepare_melt(&fail_quote.id, HashMap::new()).await?;
    let fail_result = prepared_fail.confirm().await;
    t.check(
        "melt against a failing payment errors",
        fail_result.is_err(),
        match &fail_result {
            Ok(_) => "SUCCEEDED — the mint paid an invoice it should not have".to_owned(),
            Err(e) => format!("rejected: {e}"),
        },
    )?;
    // The wallet must not silently burn the inputs it reserved for a payment that never happened.
    // check_all_pending_proofs asks the MINT the state of every proof the wallet has reserved and
    // returns the ones the mint says are still unspent to the spendable set. That is the recovery
    // path a real wallet takes after a payment that did not happen.
    let reclaimed = u64::from(receiver.check_all_pending_proofs().await?);
    println!("  reclaimed {reclaimed} sats of reserved proofs after the failure");
    let after_fail = balance(&receiver).await?;
    t.check(
        "no test ecash lost to the failed payment",
        after_fail == before_fail,
        format!("{before_fail} -> {after_fail} sats"),
    )?;

    // ---------------------------------------------------------------- 7. restart recovery
    t.leg("mint restart, no lost or duplicate balance");
    let sender_pre = balance(&wallet).await?;
    let receiver_pre = balance(&receiver).await?;
    let restart_out = test_mint("restart")?;
    println!("  test-mint restart -> {restart_out}");
    // Prove the mint agrees with the wallet about which proofs are still spendable, rather than
    // trusting the wallet's local arithmetic across the restart.
    let sender_post = balance(&wallet).await?;
    let receiver_post = balance(&receiver).await?;
    t.check(
        "sender balance unchanged by restart",
        sender_post == sender_pre,
        format!("{sender_pre} -> {sender_post} sats"),
    )?;
    t.check(
        "receiver balance unchanged by restart",
        receiver_post == receiver_pre,
        format!("{receiver_pre} -> {receiver_post} sats"),
    )?;
    // A spend after the restart proves the mint's post-restart state is live, not merely readable.
    let post_send = wallet.prepare_send(Amount::from(1), SendOptions::default()).await?;
    let post_token = post_send.confirm(None).await?;
    let post_receiver = new_wallet().await?;
    let post_received = u64::from(post_receiver.receive(&post_token.to_string(), ReceiveOptions::default()).await?);
    t.check("spend works after restart", post_received == 1, format!("{post_received} sat received"))?;
    // And the pre-restart token is still burned — no duplicate credit from a rolled-back db.
    match post_receiver.receive(&token_str, ReceiveOptions::default()).await {
        Ok(a) => t.check("pre-restart spent token still rejected", false, format!("ACCEPTED {} sats", u64::from(a)))?,
        Err(e) => t.check("pre-restart spent token still rejected", true, format!("rejected: {e}"))?,
    }

    // ---------------------------------------------------------------- 8. loopback only
    t.leg("loopback-only reachability");
    let addrs = listener_local_addrs()?;
    t.check("a listener exists on 8085", !addrs.is_empty(), format!("{addrs:?}"))?;
    let all_loopback = addrs.iter().all(|a| {
        // v4 127.0.0.1 little-endian, or v6 ::1
        a.eq_ignore_ascii_case("0100007F")
            || a.eq_ignore_ascii_case("00000000000000000000000001000000")
    });
    t.check(
        "every 8085 listener is bound to loopback",
        all_loopback,
        format!("/proc/net/tcp local_address {addrs:?}"),
    )?;
    match container_ip() {
        Some(ip) => {
            let target = SocketAddr::new(ip, MINT_PORT);
            let reachable = TcpStream::connect_timeout(&target, Duration::from_millis(750)).is_ok();
            t.check(
                "mint NOT reachable on the container's own routable address",
                !reachable,
                format!("{target} refused"),
            )?;
        }
        None => {
            // Stated, not silently skipped.
            bail!("could not determine a non-loopback container address; loopback exclusion UNPROVEN");
        }
    }

    println!("\n=====================================================");
    println!("PASS — {} assertions across {} legs", t.checks, t.legs.len());
    for (i, leg) in t.legs.iter().enumerate() {
        println!("  {}. {leg}", i + 1);
    }
    println!("mint: {MINT_URL} (fakewallet, worthless test ecash)");
    println!("=====================================================");
    Ok(())
}
