//! mint-acceptance — the named acceptance gate for the sandbox-local fakewallet test mint.
//!
//! Every check asserts a NUMBER or an EXACT protocol outcome, never merely "the call returned".
//! A no-op pass is a failure here: if a leg cannot be run, the harness bails rather than printing
//! a green tick.
//!
//! Two things the first version got wrong, both found by the independent review and both fixed
//! here, because they are the difference between coverage and the appearance of coverage:
//!
//!   * negative legs accepted ANY error. A transport failure is not proof that a double spend was
//!     rejected, nor that a payment did not happen. Every negative leg now matches the EXACT cdk
//!     error variant it claims (`Error::TokenAlreadySpent`, `Error::PaymentFailed`).
//!   * post-restart "no loss" was a local tautology: `total_balance` reads the wallet's own
//!     localstore (`wallet/balance.rs:10-20`), not mint state, so comparing it either side of a
//!     mint restart proves nothing about the mint. The restart leg now SPENDS THE WHOLE residual
//!     balance against the restarted mint and reconciles the received total exactly.
//!
//! Recovery is likewise exercised from a genuinely non-empty unresolved state, across a wallet
//! restart backed by a real sqlite file, not with in-memory stores that never reopen.
//!
//! Worthless test ecash. This binary talks to http://127.0.0.1:8085 and nothing else.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use cdk::nuts::nut00::ProofsMethods;
use cdk::nuts::{CurrencyUnit, MeltQuoteState, PaymentMethod};
use cdk::wallet::{ReceiveOptions, SendOptions, Wallet};
use cdk::{Amount, Error as CdkError};
use cdk_fake_wallet::{create_fake_invoice, FakeInvoiceDescription};
use cdk_sqlite::wallet::{memory, WalletSqliteDatabase};
use rand::random;

const MINT_URL: &str = "http://127.0.0.1:8085";
const MINT_PORT: u16 = 8085;

struct Tally {
    checks: u32,
    legs: Vec<String>,
}

impl Tally {
    fn new() -> Self {
        Self { checks: 0, legs: Vec::new() }
    }

    fn check(&mut self, what: &str, ok: bool, detail: impl AsRef<str>) -> Result<()> {
        self.checks += 1;
        let detail = detail.as_ref();
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

async fn memory_wallet() -> Result<Wallet> {
    let store = Arc::new(memory::empty().await?);
    Ok(Wallet::new(MINT_URL, CurrencyUnit::Sat, store, random::<[u8; 64]>(), None)?)
}

/// A wallet on a real sqlite FILE, so it can be dropped and reopened — a wallet restart.
async fn file_wallet(path: &str, seed: [u8; 64]) -> Result<Wallet> {
    let store = Arc::new(WalletSqliteDatabase::new(path).await?);
    Ok(Wallet::new(MINT_URL, CurrencyUnit::Sat, store, seed, None)?)
}

async fn balance(w: &Wallet) -> Result<u64> {
    Ok(u64::from(w.total_balance().await?))
}

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

/// Send the wallet's ENTIRE spendable balance, net of the fee CDK quotes for it, and return the
/// token plus the amount actually locked into it. This is what makes the restart leg meaningful:
/// the mint has to honour every remaining proof, not just one sat of them.
async fn send_everything(w: &Wallet) -> Result<Option<(String, u64)>> {
    let have = balance(w).await?;
    if have == 0 {
        return Ok(None);
    }
    // Walk down from the full balance: the largest sendable amount is `balance - fee`, and the fee
    // depends on the proof selection, so ask CDK rather than guessing.
    for amount in (1..=have).rev() {
        match w.prepare_send(Amount::from(amount), SendOptions::default()).await {
            Ok(prepared) => {
                let locked = u64::from(prepared.amount());
                let token = ***;
                return Ok(Some((token.to_string(), locked)));
            }
            Err(_) => continue,
        }
    }
    Ok(None)
}

/// True when this is the exact cdk error the leg claims. `matches!` on the variant, not a substring
/// of the message: an error message can change wording without changing meaning, and a substring
/// match would also accept an unrelated error that happens to contain the phrase.
fn is_already_spent(e: &CdkError) -> bool {
    matches!(e, CdkError::TokenAlreadySpent)
}

fn is_payment_failed(e: &CdkError) -> bool {
    matches!(e, CdkError::PaymentFailed)
}

/// The mint's own listening socket, read from the kernel rather than inferred from config.
/// /proc/net/tcp local_address for a loopback bind is `0100007F` (127.0.0.1, little-endian hex);
/// a wildcard bind would be `00000000`.
fn listener_local_addrs() -> Result<Vec<String>> {
    let mut found = Vec::new();
    let want_port = format!("{MINT_PORT:04X}");
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        for line in text.lines().skip(1) {
            let mut cols = line.split_whitespace();
            let _sl = cols.next();
            let Some(local) = cols.next() else { continue };
            let Some(_rem) = cols.next() else { continue };
            let Some(state) = cols.next() else { continue };
            if state != "0A" {
                continue; // 0A = TCP_LISTEN
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

/// This container's own routable address, without sending a packet.
fn container_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    sock.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_loopback())
}

fn test_mint(args: &[&str]) -> Result<String> {
    let out = Command::new("test-mint").args(args).output().context("run test-mint")?;
    if !out.status.success() {
        bail!("test-mint {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("mint-acceptance — sandbox-local fakewallet mint at {MINT_URL}");
    println!("WORTHLESS TEST ECASH. No real funds, no external payment.\n");

    let mut t = Tally::new();

    // Private scratch for the file-backed wallet. Deliberately under the mint's state root, which
    // the controller guarantees is OUTSIDE the delivered workdir.
    let state_root =
        std::env::var("TEST_MINT_STATE_ROOT").unwrap_or_else(|_| "/var/lib/cashu-test-state".into());
    let wallet_dir = format!("{state_root}/acceptance-wallet");
    std::fs::create_dir_all(&wallet_dir)?;
    let wallet_db = format!("{wallet_dir}/recovery.sqlite");
    let _ = std::fs::remove_file(&wallet_db);

    // ------------------------------------------------------------------ 1. mint info
    t.leg("mint info (NUT-06)");
    let wallet = memory_wallet().await?;
    let info = wallet.fetch_mint_info().await?.ok_or_else(|| anyhow!("mint returned no info"))?;
    let version = info.version.as_ref().map(|v| v.to_string()).unwrap_or_default();
    t.check("mint version is exactly cdk-mintd/0.17.2", version == "cdk-mintd/0.17.2", &version)?;
    t.check(
        "mint advertises NUT-04 mint support",
        !info.nuts.nut04.methods.is_empty(),
        format!("{} method(s)", info.nuts.nut04.methods.len()),
    )?;

    // ------------------------------------------------------------------ 2. issue
    t.leg("mint quote + issue");
    const ISSUE: u64 = 64;
    let minted = issue(&wallet, ISSUE).await?;
    t.check("proofs minted", minted == ISSUE, format!("{minted} sats (wanted {ISSUE})"))?;
    let after_issue = balance(&wallet).await?;
    t.check("wallet balance after issue", after_issue == ISSUE, format!("{after_issue} sats"))?;

    // ------------------------------------------------------------------ 3. send / receive
    t.leg("send + receive across wallets");
    const SEND: u64 = 21;
    let prepared = wallet.prepare_send(Amount::from(SEND), SendOptions::default()).await?;
    let send_fee = u64::from(prepared.fee());
    let token_str = ***.to_string();
    t.check(
        "token is a cashu token",
        token_str.starts_with("cashu"),
        format!("{} chars", token_str.len()),
    )?;
    let sender_after = balance(&wallet).await?;
    let expect_sender = ISSUE - SEND - send_fee;
    t.check(
        "sender debited exactly amount+fee",
        sender_after == expect_sender,
        format!("{sender_after} sats (issued {ISSUE} - sent {SEND} - fee {send_fee})"),
    )?;

    let receiver = memory_wallet().await?;
    let received = u64::from(receiver.receive(&token_str, ReceiveOptions::default()).await?);
    t.check("receiver credited", received == SEND, format!("{received} sats (wanted {SEND})"))?;

    // ------------------------------------------------------------------ 4. double spend
    t.leg("double spend rejected with the exact protocol error");
    let thief = memory_wallet().await?;
    match thief.receive(&token_str, ReceiveOptions::default()).await {
        Ok(a) => t.check(
            "second receive of the same token",
            false,
            format!("ACCEPTED {} sats — double spend!", u64::from(a)),
        )?,
        Err(e) => t.check(
            "second receive fails with Error::TokenAlreadySpent",
            is_already_spent(&e),
            format!("{e:?}"),
        )?,
    }
    let thief_bal = balance(&thief).await?;
    t.check("double-spender balance stays zero", thief_bal == 0, format!("{thief_bal} sats"))?;

    // ------------------------------------------------------------------ 5. melt
    t.leg("melt");
    const MELT: u64 = 5;
    let good_invoice =
        create_fake_invoice(MELT * 1_000, serde_json::to_string(&FakeInvoiceDescription::default())?);
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
        "melt finalised Paid",
        format!("{:?}", confirmed.state()).eq_ignore_ascii_case("Paid"),
        format!("{:?}", confirmed.state()),
    )?;
    t.check(
        "balance fell by exactly amount+fee",
        after_melt == before_melt - MELT - fee_paid,
        format!("{before_melt} -> {after_melt} (melt {MELT}, fee {fee_paid})"),
    )?;

    // ------------------------------------------------------------------ 6. failed payment
    // cdk-fake-wallet reads the invoice DESCRIPTION as a FakeInvoiceDescription. `pay_err` alone is
    // NOT enough: the backend records `check_payment_state` into its payment-states map BEFORE it
    // honours pay_err (cdk-fake-wallet 0.17.2 src/lib.rs:706-714), so with the struct's Paid
    // default the mint's follow-up status check reports PAID and the melt finalises Paid for an
    // invoice the backend refused. Both states must say Unpaid.
    t.leg("failed payment: exact error, terminal quote state, nothing lost");
    let fail_desc = FakeInvoiceDescription {
        pay_invoice_state: MeltQuoteState::Unpaid,
        check_payment_state: MeltQuoteState::Unpaid,
        pay_err: true,
        check_err: false,
    };
    let bad_invoice = create_fake_invoice(MELT * 1_000, serde_json::to_string(&fail_desc)?);
    let before_fail = balance(&receiver).await?;
    let fail_quote = receiver
        .melt_quote(PaymentMethod::BOLT11, bad_invoice.to_string(), None, None)
        .await?;
    let fail_quote_id = fail_quote.id.clone();
    let prepared_fail = receiver.prepare_melt(&fail_quote_id, HashMap::new()).await?;
    match prepared_fail.confirm().await {
        Ok(f) => t.check(
            "melt against a refusing backend",
            false,
            format!("SUCCEEDED state={:?} amount={}", f.state(), u64::from(f.amount())),
        )?,
        Err(e) => t.check(
            "melt fails with Error::PaymentFailed",
            is_payment_failed(&e),
            format!("{e:?}"),
        )?,
    }
    // Reconcile the QUOTE's terminal state with the mint, not just the local error.
    let quote_state = receiver.check_melt_quote_status(&fail_quote_id).await?;
    let quote_state_name = format!("{:?}", quote_state.state);
    t.check(
        "mint reports the failed quote as Unpaid",
        quote_state.state == MeltQuoteState::Unpaid,
        &quote_state_name,
    )?;
    let after_fail = balance(&receiver).await?;
    let pending_after_fail = u64::from(receiver.total_pending_balance().await?);
    let reserved_after_fail = u64::from(receiver.total_reserved_balance().await?);
    t.check(
        "no test ecash lost to the failed payment",
        after_fail == before_fail,
        format!("{before_fail} -> {after_fail} sats"),
    )?;
    t.check(
        "nothing left stranded pending or reserved",
        pending_after_fail == 0 && reserved_after_fail == 0,
        format!("pending {pending_after_fail}, reserved {reserved_after_fail}"),
    )?;

    // ------------------------------------------------------------------ 7. recovery
    // A genuinely non-empty unresolved state, across a WALLET restart. `confirm()` on a send leaves
    // the saga in TokenCreated with the proofs committed to a token nobody has redeemed; dropping
    // the wallet and reopening the same sqlite FILE is the restart. Then recovery must find it.
    t.leg("recovery of an unresolved send across a wallet restart");
    let seed: [u8; 64] = random();
    const RECOVER_FUND: u64 = 32;
    const RECOVER_SEND: u64 = 12;
    let stranded_amount;
    {
        let w = file_wallet(&wallet_db, seed).await?;
        let funded = issue(&w, RECOVER_FUND).await?;
        t.check("recovery wallet funded", funded == RECOVER_FUND, format!("{funded} sats"))?;
        let prep = w.prepare_send(Amount::from(RECOVER_SEND), SendOptions::default()).await?;
        stranded_amount = u64::from(prep.amount());
        // Token created and deliberately NEVER redeemed: this is the unresolved state.
        let _token = ***;
        let pending_sends = w.get_pending_sends().await?;
        t.check(
            "an unresolved send exists before restart",
            pending_sends.len() == 1,
            format!("{} pending send(s), {stranded_amount} sats locked", pending_sends.len()),
        )?;
        // w drops here: wallet process gone, sqlite file left as it was.
    }

    let w2 = file_wallet(&wallet_db, seed).await?;
    let pending_after_restart = w2.get_pending_sends().await?;
    t.check(
        "the unresolved send survives the wallet restart",
        pending_after_restart.len() == 1,
        format!("{} pending send(s) after reopen", pending_after_restart.len()),
    )?;
    let report = w2.recover_incomplete_sagas().await?;
    t.check(
        "recovery reports a non-empty result",
        !report.is_empty(),
        format!(
            "recovered {}, compensated {}, skipped {}, failed {}",
            report.recovered, report.compensated, report.skipped, report.failed
        ),
    )?;
    t.check("recovery failed nothing", report.failed == 0, format!("failed {}", report.failed))?;

    // Reclaim it explicitly. revoke_send swaps the proofs back — this is the call that actually
    // returns value to the spendable set, which is what `check_all_pending_proofs` does NOT do.
    let still_pending = w2.get_pending_sends().await?;
    let balance_before_revoke = balance(&w2).await?;
    if let Some(op) = still_pending.first().copied() {
        let reclaimed = u64::from(w2.revoke_send(op).await?);
        let balance_after_revoke = balance(&w2).await?;
        t.check(
            "revoke_send reclaims the stranded value",
            reclaimed > 0,
            format!("reclaimed {reclaimed} sats of {stranded_amount} locked"),
        )?;
        t.check(
            "spendable balance grows by exactly the reclaimed amount",
            balance_after_revoke == balance_before_revoke + reclaimed,
            format!("{balance_before_revoke} -> {balance_after_revoke} (+{reclaimed})"),
        )?;
        t.check(
            "no unresolved sends remain",
            w2.get_pending_sends().await?.is_empty(),
            format!("{} left", w2.get_pending_sends().await?.len()),
        )?;
    } else {
        // Recovery compensated it by itself: assert that outcome instead of claiming the other one.
        t.check(
            "recovery compensated the send without an explicit revoke",
            report.compensated >= 1,
            format!("compensated {}", report.compensated),
        )?;
    }
    // What check_all_pending_proofs ACTUALLY does, asserted rather than described: it returns the
    // total of orphaned proofs still pending at the mint, and removes the spent ones. It does not
    // move survivors back to Unspent (cdk 0.17.2 src/wallet/proofs.rs:121-180).
    let still_pending_amount = u64::from(w2.check_all_pending_proofs().await?);
    t.check(
        "check_all_pending_proofs returns the residual pending total",
        still_pending_amount == u64::from(w2.total_pending_balance().await?),
        format!(
            "returned {still_pending_amount}, wallet pending {}",
            u64::from(w2.total_pending_balance().await?)
        ),
    )?;

    // ------------------------------------------------------------------ 8. mint restart
    t.leg("mint restart: full residual value still spendable, nothing duplicated");
    let sender_pre = balance(&wallet).await?;
    let receiver_pre = balance(&receiver).await?;
    let recovery_pre = balance(&w2).await?;
    let total_pre = sender_pre + receiver_pre + recovery_pre;
    t.check(
        "there is real residual value to test with",
        total_pre > 0,
        format!("sender {sender_pre} + receiver {receiver_pre} + recovery {recovery_pre} = {total_pre}"),
    )?;
    println!("  {}", test_mint(&["restart"])?.replace('\n', " | "));

    // Now prove it against the MINT: sweep every wallet's whole balance into a fresh wallet. Local
    // totals cannot lie about this, because the mint has to sign every swap.
    let sink = memory_wallet().await?;
    let mut swept = 0u64;
    let mut fees = 0u64;
    for (name, w) in [("sender", &wallet), ("receiver", &receiver), ("recovery", &w2)] {
        let before = balance(w).await?;
        match send_everything(w).await? {
            Some((token, locked)) => {
                let got = u64::from(sink.receive(&token, ReceiveOptions::default()).await?);
                t.check(
                    &format!("{name}: mint honoured the swept token after restart"),
                    got == locked,
                    format!("swept {got} of {before} sats"),
                )?;
                swept += got;
                fees += before - locked;
            }
            None => t.check(
                &format!("{name}: nothing to sweep"),
                before == 0,
                format!("{before} sats but no sendable amount"),
            )?,
        }
    }
    let sink_balance = balance(&sink).await?;
    t.check(
        "swept value reconciles exactly with the pre-restart total",
        sink_balance == swept && swept + fees == total_pre,
        format!("sink {sink_balance} = swept {swept}; swept + fees {fees} = {total_pre} pre-restart"),
    )?;
    for (name, w) in [("sender", &wallet), ("receiver", &receiver), ("recovery", &w2)] {
        let left = balance(w).await?;
        let pending = u64::from(w.total_pending_balance().await?);
        t.check(
            &format!("{name} fully drained, nothing stranded"),
            left == 0 && pending == 0,
            format!("spendable {left}, pending {pending}"),
        )?;
    }
    // No duplicate credit from a rolled-back database: the token spent before the restart is still
    // burned, with the exact protocol error.
    match sink.receive(&token_str, ReceiveOptions::default()).await {
        Ok(a) => t.check(
            "pre-restart spent token",
            false,
            format!("ACCEPTED {} sats after restart — duplicate credit!", u64::from(a)),
        )?,
        Err(e) => t.check(
            "pre-restart spent token still fails with Error::TokenAlreadySpent",
            is_already_spent(&e),
            format!("{e:?}"),
        )?,
    }

    // ------------------------------------------------------------------ 9. loopback only
    t.leg("loopback-only reachability");
    let addrs = listener_local_addrs()?;
    t.check("a listener exists on 8085", !addrs.is_empty(), format!("{addrs:?}"))?;
    let all_loopback = addrs.iter().all(|a| {
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
        None => bail!("could not determine a non-loopback container address; exclusion UNPROVEN"),
    }

    // ------------------------------------------------------------------ 10. state isolation
    // The mint's private state must not be inside the delivered workdir (advisor F3). The
    // controller owns this check; the gate runs it so a single command covers it.
    t.leg("private test state is outside the delivered workdir");
    let isolation = test_mint(&["isolation"])?;
    for line in isolation.lines() {
        println!("  | {line}");
    }
    t.check(
        "test-mint isolation reports state outside the delivery dir",
        isolation.contains("state inside delivered dir: no"),
        "see the lines above",
    )?;
    t.check(
        "no mint state names appear in the delivered workdir",
        !isolation.contains("FOUND:"),
        "see the lines above",
    )?;

    println!("\n=====================================================");
    println!("PASS — {} assertions across {} legs", t.checks, t.legs.len());
    for (i, leg) in t.legs.iter().enumerate() {
        println!("  {}. {leg}", i + 1);
    }
    println!("mint: {MINT_URL} (fakewallet, worthless test ecash)");
    println!("=====================================================");
    Ok(())
}
