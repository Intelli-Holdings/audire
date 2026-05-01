# Audire billing model

> **Status:** locked for v1 of Audire Sync. Reviewed 2026-05-01.

## TL;DR

Audire uses **BYOK (bring your own keys)** across every tier — Free, Sync,
and Sync Plus. Audire never charges for AI usage and never proxies
audio or LLM traffic. You pay providers (Deepgram, AssemblyAI, OpenAI,
Anthropic, Google, Ollama) directly using your own account; we charge
only for the optional Sync service that stores your end-to-end encrypted
notes and transcripts.

This document explains *why* we picked BYOK over a managed-credits
model, and the conditions under which we'd revisit the decision.

## What we picked

| Tier        | Local app | Sync (E2E encrypted) | ASR / LLM keys |
| ----------- | --------- | -------------------- | -------------- |
| Free        | ✅        | ❌                   | BYOK           |
| Sync $4     | ✅        | ✅ (1 vault, 1 GB)   | BYOK           |
| Sync Plus $8| ✅        | ✅ (10 vaults, 10 GB)| BYOK           |

The pricing page on the marketing site (`website/pricing.html`) is the
canonical user-facing copy.

## Why BYOK

1. **Privacy story stays airtight.** With BYOK, audio and prompts go
   from the user's device straight to the provider over TLS. Audire's
   infrastructure has no way to see, log, or retain the content. With
   managed credits, we'd be a billing intermediary which means
   provider-side usage logs would be tied to our account — a real,
   non-zero data-access surface that we'd have to defend.
2. **No margin pressure on launch.** Provider pricing changes faster than
   we could re-bill subscribers. By keeping Audire's bill scoped to
   Sync (storage + bandwidth on Fly + Neon — predictable), we avoid the
   "Deepgram raised prices, do we eat it or pass it on?" treadmill.
3. **Faster time to ship.** Managed credits would require: Stripe
   metered billing, a key-broker service that vends short-lived
   per-user provider tokens, per-user rate limits, usage-log
   reconciliation, abuse detection, and refund flow. That's months of
   work that delays the actual value proposition (encrypted sync).
4. **Aligned with the rest of the product.** Local-first, open source,
   no telemetry, OS-keyring-stored secrets — BYOK is the consistent
   choice.

## Why we did not pick managed credits

A managed-credits tier ("we include $X of ASR usage per month") is
attractive because it's a real subscription business with margin. We
rejected it for v1 because:

- It changes the privacy story in a non-cosmetic way.
- It needs Stripe metered billing + an internal key broker before any
  user can record — meaningful infrastructure debt.
- The market shows mixed results: Otter charges for usage; Granola is
  Anthropic-funded and absorbs LLM cost; both bear ongoing variable
  cost we'd struggle to underwrite while bootstrapped.

## When we'd revisit (Phase 5+ option)

We've left the door open to add an "Audire-managed AI" tier later (call
it `Sync Pro`, ~$25/seat/mo) that includes a monthly credit pool, *if
and only if* all of the following are true:

- Audire Sync has > 1k paying seats and stable churn — i.e., we have
  the cashflow to absorb provider variance.
- We can negotiate an enterprise contract with at least one ASR
  provider that gives us billing-level discounts (>30%) on our
  aggregated usage.
- We've built the broker service to a level where adding it does not
  delay any other roadmap item.
- The privacy delta from "we have provider-side usage logs" can be
  surfaced clearly in the in-app upgrade flow, and the BYOK tiers
  remain available as the privacy-default option.

If we ever ship that tier, this document gets updated and the privacy
policy gets a new section. The BYOK tiers do not go away.

## Implementation notes

- `src-tauri/src/keyvault/` is the only place ASR/LLM keys live.
- `src-tauri/src/ipc.rs::start_capture` reads keys via
  `state.keyvault.get_provider_key(provider)`. There is **no** code
  path that fetches a key from any Audire-operated server.
- A pre-flight check in `src/sidebar.js::startCapture` short-circuits
  the recording flow if the user has not added an ASR key, jumping
  them straight to Settings → API Keys.
- Settings → API Keys (`src/views/settings.js::renderConnectorsSection`)
  groups ASR (required) above LLM (optional), shows an empty-state
  callout for first-run users, and links to each provider's signup
  page.

## Audit trail

- 2026-05-01 — picked BYOK over managed credits. Documented in this
  file and on `audire.app/pricing`.
