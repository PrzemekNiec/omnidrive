# OmniDrive - Project Status & Roadmap

## Vision

OmniDrive is a local-first, zero-knowledge Windows storage product that combines:
- one logical vault
- on-demand access in Explorer
- multiple cloud backends
- resilient recovery, repair, and maintenance tooling

Current product direction:
- stable desktop install on Windows
- local-only onboarding first
- cloud capabilities enabled progressively after provider configuration
- operator-grade diagnostics and maintenance built into the product UI

## Current Product State

What works today:
- per-user Windows installer
- automatic runtime bootstrap under `%LOCALAPPDATA%\OmniDrive`
- stable daemon autostart
- local-only first-run mode without configured providers
- `O:\` mounted as the main user-facing vault view
- Smart Sync / SyncRoot support when cloud mode is active
- encrypted metadata backup and disaster recovery
- scrub, repair, reconciliation, and shell self-heal flows
- maintenance dashboard and storage/cost dashboard

Known product posture:
- the product is now strong as a single-device Windows desktop vault
- the biggest remaining gaps are multi-device intelligence, conflict handling, sharing, and identity

## Completed Epics

### Epic 19.5: Virtual Drive Mapping [x] Completed
- OmniDrive vault is exposed through `O:\`
- custom drive label and icon support exist
- the product now has a clear Explorer-facing entry point

### Epic 20: Disaster Recovery [x] Completed
- encrypted metadata backups are created and tracked
- restore flow works without requiring a running daemon
- recovery status is exposed through API, CLI, and UI

### Epic 21: Deep Data Scrubbing [x] Completed
- background scrubber verifies shard health
- light and deep verification modes exist
- degraded and unreadable states are detected correctly

### Epic 22: Intelligent Local Cache & Predictive Prefetching [x] Completed
- encrypted local cache exists under `%LOCALAPPDATA%\OmniDrive\Cache`
- downloader uses cache-aware reads
- LRU eviction and predictive prefetching are implemented

### Epic 23.5: Flexible Storage & Policy Reconciliation [x] Completed
- `EC_2_1`, `SINGLE_REPLICA`, and `LOCAL_ONLY` storage modes are implemented
- read path understands each mode
- reconciliation can migrate active data between storage modes

### Epic 24: Secure Local Runtime [x] Completed
- key material is better protected in memory
- cache encryption is separated from the main vault key
- sensitive runtime directories are ACL-hardened

### Epic 26: End-to-End Test Matrix [x] Completed
- recovery, reconciliation, and self-healing are validated by E2E tests
- critical storage and repair flows are no longer only unit-tested in isolation

### Epic 27: Installer and First-Run Bootstrap [x] Completed
- per-user installer is working
- installed-mode path resolution is working
- clean-machine bootstrap is stable
- local-only onboarding works without remote providers
- reboot validation passed

### Epic 28: Self-Healing Shell Integration [x] Completed
- shell state audit exists
- shell repair and sync-root repair exist
- startup recovery exists
- second-machine validation passed

### Epic 29: Storage Cost and Policy Dashboard [x] Completed
- `/api/storage/cost` exists
- storage economics are visible in the dashboard
- policy mix, provider distribution, reconcile backlog, and GC debt are visible
- acceptance passed on the test machine

### Epic 30: Maintenance Console [x] Completed
- maintenance actions are available in the dashboard
- diagnostics are aggregated in one operator-facing view
- repair, scrub, backup, and reconciliation are triggerable from UI
- acceptance passed on the test machine

## Open Epics

### Epic 31: P2P LAN Cache
Goal:
- prefer trusted local peers before cloud downloads

Scope:
- peer discovery
- mutual authentication
- LAN transfer of encrypted chunks or shards
- downloader preference for LAN before cloud

Outcome:
- lower egress cost and faster reads in home/office networks

### Epic 32: Sync Conflict Handling
Goal:
- prevent silent overwrite ambiguity across devices

Scope:
- conflict detection
- conflict naming / conflict copies
- revision ordering rules
- optional lock / lease semantics where justified

Outcome:
- safe multi-device behavior

### Epic 33: Zero-Knowledge Link Sharing
Goal:
- allow private file sharing without exposing keys to the server

Scope:
- share token generation
- dedicated download page
- URL-fragment key delivery
- browser-side decrypt / decode

Outcome:
- private zero-knowledge sharing

### Epic 34: Secure Authentication and Google Login
Goal:
- add account-backed identity only if OmniDrive grows beyond the current local-daemon product model

Scope:
- secure auth model
- Google Login / OAuth
- session lifecycle
- device association
- onboarding integration

Outcome:
- hosted or multi-user identity layer when product direction requires it

## Recommended Order

1. `Epic 31: P2P LAN Cache`
2. `Epic 32: Sync Conflict Handling`
3. `Epic 33: Zero-Knowledge Link Sharing`
4. `Epic 34: Secure Authentication and Google Login`

Why this order:
- `Epic 31` and `Epic 32` directly improve the core storage product
- `Epic 33` expands product value after the multi-device model is safer
- `Epic 34` is the most optional and should only happen when account-backed product direction is confirmed

## Historical / Superseded Planning

The old roadmap sections for earlier epics `9-19`, duplicate implementation plans, and older phased planning were intentionally removed from this file.

Reason:
- they are now either already reflected in shipped architecture
- or they are no longer the active execution plan
- keeping them here created noise, duplication, and a misleading picture of what is still actually left to build

If needed, the old detailed plans still exist in git history.
