# cdk-wallet-api — CDK 0.17.2 wallet calls that exist

Verified by compiling against `=0.17.2`. Names in this family are easy to guess wrong; these are
the ones that are real.

## Construction

```rust
let store = Arc::new(cdk_sqlite::wallet::memory::empty().await?);   // or a file store
let wallet = Wallet::new(mint_url, CurrencyUnit::Sat, store, seed_bytes /* [u8; 64] */, None)?;
```

## Issue

```rust
let quote = wallet.mint_quote(PaymentMethod::BOLT11, Some(Amount::from(64)), None, None).await?;
let proofs = wallet
    .wait_and_mint_quote(quote, Default::default(), Default::default(), Duration::from_secs(30))
    .await?;
let minted = proofs.total_amount()?;         // needs `use cdk::nuts::nut00::ProofsMethods;`
```

Against the fakewallet mint the quote auto-settles, so `wait_and_mint_quote` returns promptly.

## Send / receive

```rust
let prepared = wallet.prepare_send(Amount::from(21), SendOptions::default()).await?;
let fee = prepared.fee();
let token = prepared.confirm(None).await?;   // consumes `prepared`
let amount = other_wallet.receive(&token.to_string(), ReceiveOptions::default()).await?;
```

`prepare_send` reserves proofs; the debit is not final until `confirm`.

## Melt

```rust
let quote = wallet.melt_quote(PaymentMethod::BOLT11, invoice_string, None, None).await?;
let prepared = wallet.prepare_melt(&quote.id, HashMap::new()).await?;
let finalized = prepared.confirm().await?;                 // or .confirm_prefer_async() -> MeltOutcome
finalized.state(); finalized.amount(); finalized.fee_paid();
```

`confirm_prefer_async()` returns `MeltOutcome::{Paid, Pending}`; a `Pending` can be awaited
directly, or resolved later with `wallet.finalize_pending_melts()` /
`wallet.check_melt_quote_status(&id)`.

## Balance and recovery

- `wallet.total_balance()` — spendable.
- `wallet.total_pending_balance()` — reserved.
- `wallet.check_all_pending_proofs()` — asks the MINT the state of every reserved proof and returns
  the still-unspent ones to the spendable set. This is the recovery path after a payment that did
  not happen. Returns the reclaimed `Amount`.
- `wallet.get_pending_proofs()`, `wallet.get_pending_spent_proofs()`, `wallet.get_pending_sends()`.

## Mint info — the name trap

It is **`wallet.fetch_mint_info()`** (returns `Result<Option<MintInfo>, Error>`), not
`get_mint_info` — that name exists only on `AuthWallet`. There is also `wallet.load_mint_info()`
returning `MintInfo` directly.

There is no `reclaim_unspent_proofs()`; the call is `check_all_pending_proofs()`.

## Failure signatures worth recognising

- Receiving an already-spent token: `Error` displaying **`Token Already Spent`**.
- A melt whose backend refuses: the wallet call errors with **`Payment failed`**. A melt can also
  come back `Ok` with a non-`Paid` state — assert on the state, not merely on `is_ok()`.
