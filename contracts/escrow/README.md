# astroid-escrow

Escrow contract — temporary custody until a designated arbiter resolves the
release condition. A single agreement can hold several distinct Stellar
assets, and can optionally be released early by a set of pre-configured
ed25519 signers instead of the named arbiter.

```text
funder ──► create(sender, recipient, arbiter, assets[], deadline, memo,
                   release_signers[], release_threshold) ──► Escrow-Funded
                      │
                      ├─► arbiter.release(id)                     ──► Released ──► assets move
                      ├─► override_release(id, nonce, signatures) ──► Released ──► assets move
                      └─► sender.refund(id)                       ──► Refunded   (after deadline)
```

## State machine

`Created → Funded → (Released | Refunded | Expired) → Closed`

- `create` funds immediately (atomic in a single call), pulling every listed
  `(asset, amount)` pair into custody.
- `release` requires the arbiter and a live deadline.
- `override_release` requires at least `release_threshold` distinct, valid
  ed25519 signatures from `release_signers`, each over a deterministic
  payload (contract address, network id, escrow id, nonce), and a live
  deadline. It is permissionless — the signatures are the authorization, so
  any relayer may submit them. Pass an empty signer set (and threshold `0`)
  at `create` time to disable this path for an escrow.
- `refund` is permissionless after the deadline — a beneficiary that never
  claims and an absent arbiter default back to the funder.
- `close` (terminal) requires one of the three roles once the escrow is final.

## Invariants

- Caller must be the recorded role for `release` / `refund` / `close`.
- Every asset amount must be positive (shared `require_positive_amount`), the
  asset list may not be empty, exceed `MAX_ESCROW_ASSETS`, or repeat an asset.
- Releasing after the deadline auto-marks the escrow `Expired` and aborts.
- Override signatures must come from distinct, pre-configured signers and
  meet the threshold; a nonce is only accepted once and must strictly
  increase per escrow, which makes a captured signature set unusable a
  second time (replay protection).
- `EscrowReleased` is emitted (via the shared, structured event schema)
  detailing the escrow id, recipient and every asset transferred, on both the
  arbiter and signature-override release paths.

## Use-cases

- Milestone payments between ON-CHAIN purchased services.
- Agent-to-agent micro-settlement with audit trail.
- Marketplace / freelance payouts where a human arbiter adjudicates.
