# OmniDrive: Architect's State Ledger (v0.1.15)
*Ostatnia aktualizacja: 2026-04-06*

## 🏗️ Context & Role
- **System:** OmniDrive (Local-First, Zero-Knowledge Storage).
- **Core:** Rust daemon (`angeld`), SQLite, Windows VFS (O:\), Cloud Storage (R2, B2, Scaleway).
- **Current Role:** Senior Software Architect.
- **Project Paradigm:** "Product-First" (stable, usable) over "Spec-First" (formal repository models).

## 🚀 Current Milestone: v0.1.15
- **Critical Fix:** Grafting provider configurations on new machines (e.g., Dell).
- **Fix Detail:** `db.rs` now handles `INSERT OR IGNORE` for `provider_configs` during graft, with `enabled=0` flag and missing secret reporting via `VaultRestoreApplyReport`.
- **UX Improvement:** Hot-reload of providers via POST `/api/onboarding/setup-provider`.

## 🗺️ Roadmap v2.0 Strategy
- **Phase 1 (Epic 32.5):** Envelope Encryption (DEK -> Vault Key -> KDF). *Next Step: Cryptographic Decision Document (Phase 0).*
- **Phase 2 (Epic 35):** Ghost Shell / CFAPI Integration. *Critical Risk: `cfapi.dll` PoC stability.*
- **Phase 3 (Epic 33):** Zero-Knowledge Link Sharing (Fragment-based URI).
- **Phase 4 (Epic 34):** Family Cloud (OAuth Identity vs X25519 Cryptography).

## ⚠️ Architectural Risks & Debts
1. **Multi-device Conflict:** Lack of formal Lease/Fencing model (from Spec v1). Priority: Epic 32.
2. **Platform Dependency:** Deep Windows integration (Explorer/CFAPI) increases crash risk.
3. **Repository Format:** Deviation from "Canonical Object Repository" (SQLite as primary truth).

## 📄 Key Files Reference
- History: `omnidrive_chat_part1-3.docx` (1400+ pages of context).
- Review: `spec_review.md` (Product vs Spec analysis).
- Roadmap: `OmniDrive_Roadmap_v2.md` (Future plans).
