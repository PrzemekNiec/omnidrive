use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Expect {
    /// Celowo publiczne — lista i uzasadnienie w §9.5 ARCHITECTURE.md.
    Public,
    /// Publiczne, ale wymaga nagłówka X-OmniDrive-Local (anty-CSRF, Zadanie 11).
    LocalIntent,
    /// Wymaga dowolnej ważnej sesji.
    Session,
    /// Wymaga sesji z rolą w vaulcie.
    Role,
    /// Otwarta wyłącznie dopóki `onboarding_state != COMPLETED`; po zakończeniu
    /// kreatora wymaga Admina (`AdminAfterOnboarding`). Dowód dla tego wariantu
    /// niesie `setup_provider_is_open_during_onboarding_and_gated_after`.
    OpenDuringOnboarding,
}

const AUTH_MATRIX: &[(&str, &str, &str, Expect)] = &[
    (
        "GET",
        "/api/vault/status",
        "/api/vault/status",
        Expect::Public,
    ),
    ("POST", "/api/unlock", "/api/unlock", Expect::Public),
    (
        "GET",
        "/api/unlock/hello-available",
        "/api/unlock/hello-available",
        Expect::Public,
    ),
    ("POST", "/api/vault/join", "/api/vault/join", Expect::Public),
    (
        "POST",
        "/api/recovery/restore",
        "/api/recovery/restore",
        Expect::Public,
    ),
    (
        "GET",
        "/api/recovery/status",
        "/api/recovery/status",
        Expect::Public,
    ),
    (
        "GET",
        "/api/onboarding/status",
        "/api/onboarding/status",
        Expect::Public,
    ),
    ("GET", "/api/health", "/api/health", Expect::Public),
    (
        "GET",
        "/api/diagnostics/health",
        "/api/diagnostics/health",
        Expect::Public,
    ),
    (
        "POST",
        "/api/unlock/windows-hello",
        "/api/unlock/windows-hello",
        Expect::LocalIntent,
    ),
    ("GET", "/api/transfers", "/api/transfers", Expect::Role),
    ("GET", "/api/diagnostics", "/api/diagnostics", Expect::Role),
    (
        "GET",
        "/api/diagnostics/shell",
        "/api/diagnostics/shell",
        Expect::Role,
    ),
    (
        "GET",
        "/api/diagnostics/sync-root",
        "/api/diagnostics/sync-root",
        Expect::Role,
    ),
    (
        "GET",
        "/api/diagnostics/restore",
        "/api/diagnostics/restore",
        Expect::Role,
    ),
    (
        "GET",
        "/api/storage/cost",
        "/api/storage/cost",
        Expect::Role,
    ),
    (
        "GET",
        "/api/multidevice/status",
        "/api/multidevice/status",
        Expect::Role,
    ),
    (
        "GET",
        "/api/stats/overview",
        "/api/stats/overview",
        Expect::Role,
    ),
    ("GET", "/api/ingest", "/api/ingest", Expect::Role),
    (
        "POST",
        "/api/onboarding/setup-provider",
        "/api/onboarding/setup-provider",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/onboarding/complete",
        "/api/onboarding/complete",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/onboarding/reset",
        "/api/onboarding/reset",
        Expect::OpenDuringOnboarding,
    ),
    (
        "DELETE",
        "/api/onboarding/provider/{provider_name}",
        "/api/onboarding/provider/backblaze-b2",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/providers/{provider_name}/test",
        "/api/providers/backblaze-b2/test",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/vault/add-device",
        "/api/vault/add-device",
        Expect::Role,
    ),
    (
        "POST",
        "/api/vault/rotate-key",
        "/api/vault/rotate-key",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/repair-shell",
        "/api/maintenance/repair-shell",
        Expect::Role,
    ),
    (
        "POST",
        "/api/files/{inode_id}/pin",
        "/api/files/1/pin",
        Expect::Role,
    ),
    (
        "DELETE",
        "/api/files/{inode_id}",
        "/api/files/1",
        Expect::Role,
    ),
    (
        "DELETE",
        "/api/shares/{share_id}",
        "/api/shares/abc123",
        Expect::Role,
    ),
    ("GET", "/", "/", Expect::Public),
    ("GET", "/api/audit", "/api/audit", Expect::Role),
    (
        "GET",
        "/api/auth/google/callback",
        "/api/auth/google/callback",
        Expect::Public,
    ),
    (
        "GET",
        "/api/auth/google/start",
        "/api/auth/google/start",
        Expect::Public,
    ),
    (
        "GET",
        "/api/auth/session",
        "/api/auth/session",
        Expect::Public,
    ),
    (
        "GET",
        "/api/auto-lock/status",
        "/api/auto-lock/status",
        Expect::Session,
    ),
    (
        "GET",
        "/api/cache/status",
        "/api/cache/status",
        Expect::Role,
    ),
    ("GET", "/api/files", "/api/files", Expect::Role),
    (
        "GET",
        "/api/files/{inode_id}/revisions",
        "/api/files/1/revisions",
        Expect::Role,
    ),
    (
        "GET",
        "/api/files/{inode_id}/shares",
        "/api/files/1/shares",
        Expect::Role,
    ),
    (
        "GET",
        "/api/files/{inode_id}/sync_status",
        "/api/files/1/sync_status",
        Expect::Role,
    ),
    (
        "GET",
        "/api/filesystem/policies",
        "/api/filesystem/policies",
        Expect::Role,
    ),
    (
        "GET",
        "/api/health/vault",
        "/api/health/vault",
        Expect::Public,
    ),
    (
        "GET",
        "/api/maintenance/diagnostics",
        "/api/maintenance/diagnostics",
        Expect::Role,
    ),
    (
        "GET",
        "/api/maintenance/retry-storms",
        "/api/maintenance/retry-storms",
        Expect::Role,
    ),
    (
        "GET",
        "/api/maintenance/scrub-errors",
        "/api/maintenance/scrub-errors",
        Expect::Role,
    ),
    (
        "GET",
        "/api/maintenance/scrub-status",
        "/api/maintenance/scrub-status",
        Expect::Role,
    ),
    (
        "GET",
        "/api/maintenance/status",
        "/api/maintenance/status",
        Expect::Role,
    ),
    (
        "GET",
        "/api/metadata-backup/status",
        "/api/metadata-backup/status",
        Expect::Role,
    ),
    ("GET", "/api/quota", "/api/quota", Expect::Role),
    (
        "GET",
        "/api/settings/paths",
        "/api/settings/paths",
        Expect::Session,
    ),
    (
        "GET",
        "/api/share/{share_id}/chunks/{chunk_index}",
        "/api/share/abc123/chunks/0",
        Expect::Public,
    ),
    (
        "GET",
        "/api/share/{share_id}/meta",
        "/api/share/abc123/meta",
        Expect::Public,
    ),
    ("GET", "/api/shares", "/api/shares", Expect::Role),
    (
        "GET",
        "/api/stats/system",
        "/api/stats/system",
        Expect::Role,
    ),
    (
        "GET",
        "/api/stats/traffic",
        "/api/stats/traffic",
        Expect::Role,
    ),
    ("GET", "/api/trash", "/api/trash", Expect::Role),
    (
        "GET",
        "/api/vault/devices",
        "/api/vault/devices",
        Expect::Role,
    ),
    (
        "GET",
        "/api/vault/my-wrapped-key",
        "/api/vault/my-wrapped-key",
        Expect::Role,
    ),
    (
        "GET",
        "/api/vault/pending-devices",
        "/api/vault/pending-devices",
        Expect::Role,
    ),
    (
        "GET",
        "/api/vault/rewrap-status",
        "/api/vault/rewrap-status",
        Expect::Role,
    ),
    (
        "GET",
        "/api/vault/safety-numbers",
        "/api/vault/safety-numbers",
        Expect::Role,
    ),
    (
        "GET",
        "/material-symbols-outlined.ttf",
        "/material-symbols-outlined.ttf",
        Expect::Public,
    ),
    ("GET", "/qrcode.min.js", "/qrcode.min.js", Expect::Public),
    ("GET", "/share-sw.js", "/share-sw.js", Expect::Public),
    ("GET", "/share/{share_id}", "/share/abc123", Expect::Public),
    (
        "GET",
        "/sw-download/{share_id}",
        "/sw-download/abc123",
        Expect::Public,
    ),
    ("GET", "/wizard", "/wizard", Expect::Public),
    ("GET", "/wizard.js", "/wizard.js", Expect::Public),
    (
        "POST",
        "/api/auth/logout",
        "/api/auth/logout",
        Expect::Session,
    ),
    (
        "POST",
        "/api/auth/renew",
        "/api/auth/renew",
        Expect::Session,
    ),
    (
        "POST",
        "/api/auto-lock/_test/simulate-session-lock",
        "/api/auto-lock/_test/simulate-session-lock",
        Expect::Session,
    ),
    (
        "POST",
        "/api/auto-lock/timeout",
        "/api/auto-lock/timeout",
        Expect::Session,
    ),
    (
        "POST",
        "/api/auto-lock/touch",
        "/api/auto-lock/touch",
        Expect::Session,
    ),
    (
        "POST",
        "/api/change-password",
        "/api/change-password",
        Expect::Session,
    ),
    (
        "POST",
        "/api/devices/{device_id}/revoke",
        "/api/devices/dev1/revoke",
        Expect::Role,
    ),
    (
        "POST",
        "/api/devices/{device_id}/verify",
        "/api/devices/dev1/verify",
        Expect::Role,
    ),
    (
        "POST",
        "/api/files/{inode_id}/revisions/{revision_id}/materialize-conflict-copy",
        "/api/files/1/revisions/1/materialize-conflict-copy",
        Expect::Role,
    ),
    (
        "POST",
        "/api/files/{inode_id}/revisions/{revision_id}/restore",
        "/api/files/1/revisions/1/restore",
        Expect::Role,
    ),
    (
        "POST",
        "/api/files/{inode_id}/share",
        "/api/files/1/share",
        Expect::Role,
    ),
    (
        "POST",
        "/api/files/{inode_id}/unpin",
        "/api/files/1/unpin",
        Expect::Role,
    ),
    (
        "POST",
        "/api/filesystem/pin",
        "/api/filesystem/pin",
        Expect::Role,
    ),
    (
        "POST",
        "/api/filesystem/set-policy",
        "/api/filesystem/set-policy",
        Expect::Role,
    ),
    (
        "POST",
        "/api/filesystem/unpin",
        "/api/filesystem/unpin",
        Expect::Role,
    ),
    (
        "POST",
        "/api/ingest/{job_id}/cleanup",
        "/api/ingest/1/cleanup",
        Expect::Role,
    ),
    (
        "POST",
        "/api/ingest/{job_id}/retry",
        "/api/ingest/1/retry",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/gc-orphans",
        "/api/maintenance/gc-orphans",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/reconcile-now",
        "/api/maintenance/reconcile-now",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/repair-now",
        "/api/maintenance/repair-now",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/repair-sync-root",
        "/api/maintenance/repair-sync-root",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/scrub-now",
        "/api/maintenance/scrub-now",
        Expect::Role,
    ),
    (
        "POST",
        "/api/maintenance/sync-upload-targets",
        "/api/maintenance/sync-upload-targets",
        Expect::Role,
    ),
    (
        "POST",
        "/api/metadata-backup/backup-now",
        "/api/metadata-backup/backup-now",
        Expect::Role,
    ),
    (
        "POST",
        "/api/metadata-backup/fetch-now",
        "/api/metadata-backup/fetch-now",
        Expect::Role,
    ),
    (
        "POST",
        "/api/metadata-backup/snapshot-local",
        "/api/metadata-backup/snapshot-local",
        Expect::Role,
    ),
    (
        "POST",
        "/api/onboarding/bootstrap-local",
        "/api/onboarding/bootstrap-local",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/onboarding/join-existing",
        "/api/onboarding/join-existing",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/onboarding/setup-identity",
        "/api/onboarding/setup-identity",
        Expect::OpenDuringOnboarding,
    ),
    (
        "POST",
        "/api/recovery/generate",
        "/api/recovery/generate",
        Expect::Role,
    ),
    (
        "POST",
        "/api/recovery/revoke",
        "/api/recovery/revoke",
        Expect::Role,
    ),
    (
        "POST",
        "/api/settings/autostart",
        "/api/settings/autostart",
        Expect::Session,
    ),
    (
        "POST",
        "/api/settings/restart-daemon",
        "/api/settings/restart-daemon",
        Expect::Session,
    ),
    (
        "POST",
        "/api/settings/windows-hello",
        "/api/settings/windows-hello",
        Expect::Role,
    ),
    (
        "POST",
        "/api/share/{share_id}/verify-password",
        "/api/share/abc123/verify-password",
        Expect::Public,
    ),
    (
        "POST",
        "/api/shares/{share_id}/revoke",
        "/api/shares/abc123/revoke",
        Expect::Role,
    ),
    (
        "POST",
        "/api/trash/{inode_id}/purge",
        "/api/trash/1/purge",
        Expect::Role,
    ),
    (
        "POST",
        "/api/trash/{inode_id}/restore",
        "/api/trash/1/restore",
        Expect::Role,
    ),
    (
        "POST",
        "/api/vault/accept-device/{device_id}",
        "/api/vault/accept-device/dev1",
        Expect::Role,
    ),
    (
        "POST",
        "/api/vault/invite",
        "/api/vault/invite",
        Expect::Role,
    ),
    ("POST", "/api/vault/lock", "/api/vault/lock", Expect::Role),
    (
        "POST",
        "/api/vault/members/{user_id}/remove",
        "/api/vault/members/user1/remove",
        Expect::Role,
    ),
];

/// Treść każdego wywołania `.route(...)`, także rozbitego na wiele linii.
fn route_calls(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut calls = Vec::new();
    let mut cursor = 0usize;

    while let Some(hit) = src[cursor..].find(".route(") {
        let start = cursor + hit + ".route(".len();
        let mut depth = 1usize;
        let mut in_string = false;
        let mut i = start;

        while i < bytes.len() && depth > 0 {
            let ch = bytes[i];
            let escaped = i > 0 && bytes[i - 1] == b'\\';
            match ch {
                b'"' if !escaped => in_string = !in_string,
                b'(' if !in_string => depth += 1,
                b')' if !in_string => depth -= 1,
                _ => {}
            }
            i += 1;
        }

        calls.push(src[start..i.saturating_sub(1)].to_string());
        cursor = i;
    }
    calls
}

fn first_string_literal(call: &str) -> Option<String> {
    let open = call.find('"')?;
    let rest = &call[open + 1..];
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

fn routes_declared_in_source() -> BTreeSet<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/api");
    let mut found = BTreeSet::new();

    for entry in std::fs::read_dir(dir).expect("api dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read source");
        for call in route_calls(&src) {
            let Some(route) = first_string_literal(&call) else {
                continue;
            };
            for verb in ["get", "post", "put", "patch", "delete"] {
                if call.contains(&format!("{verb}(")) {
                    found.insert((verb.to_ascii_uppercase(), route.clone()));
                }
            }
        }
    }
    found
}

#[test]
fn every_declared_route_has_an_entry_in_the_matrix() {
    let declared = routes_declared_in_source();
    let covered: BTreeSet<(String, String)> = AUTH_MATRIX
        .iter()
        .map(|(method, declared, _, _)| ((*method).to_string(), (*declared).to_string()))
        .collect();

    let missing: Vec<_> = declared.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "trasy bez wpisu w AUTH_MATRIX (dopisz je razem z decyzja o bramce): {missing:?}"
    );

    let stale: Vec<_> = covered.difference(&declared).collect();
    assert!(
        stale.is_empty(),
        "wpisy w AUTH_MATRIX bez odpowiadajacej trasy w kodzie: {stale:?}"
    );
}

mod common;
use common::DaemonHarness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routes_behind_a_gate_reject_requests_without_a_token()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let mut offenders = Vec::new();
    for (method, _declared, sample, expect) in AUTH_MATRIX {
        let wanted = match expect {
            Expect::Public => continue,
            // Otwarta tylko w trakcie kreatora; dowod ze po `complete` wymaga
            // Admina niesie setup_provider_is_open_during_onboarding_and_gated_after
            // (Zadanie 7 / 15a), nie ten test na swiezym, nieukonczonym daemonie.
            Expect::OpenDuringOnboarding => continue,
            // Brak naglowka X-OmniDrive-Local => 403 (Zadanie 11).
            Expect::LocalIntent => 403,
            // acl::extract_session_or_401: brak naglowka Authorization => zawsze 401.
            Expect::Session | Expect::Role => 401,
        };

        let resp = h.request_without_token(method, sample, None).await?;
        if resp.status != wanted {
            offenders.push(format!(
                "{method} {sample} -> {} (oczekiwano {wanted}) {}",
                resp.status, resp.body
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "endpointy bez poprawnej bramki:\n{}",
        offenders.join("\n")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_gate_rejects_non_member_but_allows_owner_and_logout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let owner_token = h.session_token.clone();

    let pool = h.connect_db().await?;
    let stranger_id = angeld::db::new_user_id();
    let device_id = angeld::db::new_user_id();
    angeld::db::create_user(&pool, &stranger_id, "Stranger", None, "local", None).await?;
    angeld::db::create_device(&pool, &device_id, &stranger_id, "StrangerPC", &[0u8; 32]).await?;
    let stranger_token = angeld::db::generate_session_token();
    angeld::db::create_user_session(
        &pool,
        &stranger_token,
        &stranger_id,
        &device_id,
        angeld::db::SESSION_TTL_SECONDS,
    )
    .await?;
    pool.close().await;

    h.session_token = Some(stranger_token.clone());

    let resp = h
        .request_with_token("POST", "/api/settings/restart-daemon", None)
        .await?;
    assert_eq!(
        resp.status, 403,
        "obcy token bez czlonkostwa nie moze zresetowac daemona: {}",
        resp.body
    );

    let resp = h
        .request_with_token("POST", "/api/auto-lock/touch", None)
        .await?;
    assert_eq!(
        resp.status, 403,
        "obcy token bez czlonkostwa nie moze dotknac timera auto-locka: {}",
        resp.body
    );

    let resp = h
        .request_with_token("POST", "/api/auth/logout", None)
        .await?;
    assert_ne!(
        resp.status, 403,
        "logout musi dzialac nawet dla nie-czlonka, inaczej obcy token przezyje do konca TTL: {}",
        resp.body
    );

    h.session_token = owner_token;
    let resp = h
        .request_with_token("POST", "/api/settings/restart-daemon", None)
        .await?;
    assert_ne!(
        resp.status, 403,
        "wlasciciel skarbca nie moze dostac 403 na wlasnej trasie: {}",
        resp.body
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vault_status_never_returns_a_session_token() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let body = h.get_json("/api/vault/status").await?;
    assert!(
        body.get("session_token").is_none(),
        "/api/vault/status nie moze wystawiac tokenu: {body}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn setup_provider_is_open_during_onboarding_and_gated_after()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;

    let body = serde_json::json!({
        "provider_name": "backblaze-b2",
        "endpoint": "http://127.0.0.1:1",
        "region": "eu-central-003",
        "bucket": "test"
    });

    let during = h
        .request_without_token("POST", "/api/onboarding/setup-provider", Some(&body))
        .await?;
    assert_ne!(
        during.status, 401,
        "w trakcie kreatora endpoint musi byc otwarty"
    );

    h.unlock().await?;
    h.post("/api/onboarding/complete").await?;

    let after = h
        .request_without_token("POST", "/api/onboarding/setup-provider", Some(&body))
        .await?;
    assert_eq!(
        after.status, 401,
        "po zakonczeniu onboardingu endpoint musi wymagac sesji; got {} body={}",
        after.status, after.body
    );

    let reset_after = h
        .request_without_token("POST", "/api/onboarding/reset", None)
        .await?;
    assert_eq!(
        reset_after.status, 401,
        "reset onboardingu po zakonczeniu tez musi wymagac Admina, inaczej otwiera droge ucieczki z powrotem do trybu otwartego kreatora; got {} body={}",
        reset_after.status, reset_after.body
    );

    let delete_provider_after = h
        .request_without_token("DELETE", "/api/onboarding/provider/backblaze-b2", None)
        .await?;
    assert_eq!(
        delete_provider_after.status, 401,
        "usuniecie dostawcy po zakonczeniu onboardingu musi wymagac Admina; got {} body={}",
        delete_provider_after.status, delete_provider_after.body
    );

    let test_provider_after = h
        .request_without_token("POST", "/api/providers/backblaze-b2/test", None)
        .await?;
    assert_eq!(
        test_provider_after.status, 401,
        "test dostawcy po zakonczeniu onboardingu musi wymagac Admina; got {} body={}",
        test_provider_after.status, test_provider_after.body
    );

    let bootstrap_local_after = h
        .request_without_token("POST", "/api/onboarding/bootstrap-local", None)
        .await?;
    assert_eq!(
        bootstrap_local_after.status, 401,
        "bootstrap-local po zakonczeniu onboardingu musi wymagac Admina; got {} body={}",
        bootstrap_local_after.status, bootstrap_local_after.body
    );

    let setup_identity_body = serde_json::json!({ "device_name": "Obce urzadzenie" });
    let setup_identity_after = h
        .request_without_token(
            "POST",
            "/api/onboarding/setup-identity",
            Some(&setup_identity_body),
        )
        .await?;
    assert_eq!(
        setup_identity_after.status, 401,
        "setup-identity po zakonczeniu onboardingu musi wymagac Admina; got {} body={}",
        setup_identity_after.status, setup_identity_after.body
    );

    let join_existing_body = serde_json::json!({
        "passphrase": "dowolne-haslo",
        "provider_id": "backblaze-b2"
    });
    let join_existing_after = h
        .request_without_token(
            "POST",
            "/api/onboarding/join-existing",
            Some(&join_existing_body),
        )
        .await?;
    assert_eq!(
        join_existing_after.status, 401,
        "join-existing po zakonczeniu onboardingu musi wymagac Admina; got {} body={}",
        join_existing_after.status, join_existing_after.body
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotate_key_rejects_wrong_old_passphrase() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let resp = h
        .post_json(
            "/api/vault/rotate-key",
            serde_json::json!({
                "old_passphrase": "nie-to-haslo",
                "new_passphrase": "nowe-haslo-1234"
            }),
        )
        .await?;

    assert_eq!(
        resp.status, 400,
        "rotacja ze zlym starym haslem musi byc odrzucona; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_device_requires_admin_and_never_wraps_for_unenrolled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let body = serde_json::json!({
        "user_id": "u-obcy",
        "device_id": "dev-obcy",
        "device_name": "Obce",
        "public_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    });

    let anon = h
        .request_without_token("POST", "/api/vault/add-device", Some(&body))
        .await?;
    assert_eq!(anon.status, 401, "add-device bez tokenu musi byc odrzucone");

    let authed = h.post_json("/api/vault/add-device", body).await?;
    assert!(
        !authed.body.contains("wrapped_vault_key\":\""),
        "nieznane urzadzenie nie moze dostac owinietego klucza: {}",
        authed.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrapped_key_endpoint_only_serves_the_calling_device()
-> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;

    let anonymous = h
        .request_without_token("GET", "/api/vault/my-wrapped-key?device_id=cudze", None)
        .await?;
    assert_eq!(
        anonymous.status, 401,
        "bez tokenu to 401 (acl::extract_session_or_401); got {} body={}",
        anonymous.status, anonymous.body
    );

    let authorized = h
        .request_with_token("GET", "/api/vault/my-wrapped-key?device_id=cudze", None)
        .await?;
    assert_eq!(
        authorized.status, 403,
        "z waznym tokenem, ale o cudze urzadzenie: 403; got {} body={}",
        authorized.status, authorized.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_hello_unlock_requires_local_intent_header()
-> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    let resp = h
        .request_without_token("POST", "/api/unlock/windows-hello", None)
        .await?;
    assert_eq!(
        resp.status, 403,
        "POST bez naglowka X-OmniDrive-Local musi byc odrzucony; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_wrong_passphrase_is_rate_limited() -> Result<(), Box<dyn std::error::Error>> {
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let body = serde_json::json!({ "passphrase": "zle" });

    let mut statuses = Vec::with_capacity(6);
    for _ in 0..6 {
        let resp = h
            .request_without_token("POST", "/api/unlock", Some(&body))
            .await?;
        statuses.push(resp.status);
    }

    for (i, status) in statuses.iter().take(5).enumerate() {
        assert_ne!(
            *status,
            429,
            "proba {} powinna byc zwykla odmowa (zle haslo), nie rate-limit; got {status}",
            i + 1
        );
    }
    assert_eq!(
        statuses[5], 429,
        "po piatej nieudanej probie /api/unlock musi zwrocic 429; got {}",
        statuses[5]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unlock_does_not_store_passphrase_unless_opted_in() -> Result<(), Box<dyn std::error::Error>>
{
    let mut h = DaemonHarness::spawn().await?;
    h.unlock().await?;
    let resp = h.get_json("/api/unlock/hello-available").await?;
    assert_eq!(
        resp["available"].as_bool(),
        Some(false),
        "bez wlaczenia opcji haslo nie moze trafic do Credential Managera; got {resp}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_route_is_removed() -> Result<(), Box<dyn std::error::Error>> {
    let h = DaemonHarness::spawn().await?;
    let resp = h.request_without_token("GET", "/legacy", None).await?;
    assert_eq!(
        resp.status, 404,
        "/legacy zostalo usuniete (Z11-03/Z9-15), musi zwracac 404; got {} body={}",
        resp.status, resp.body
    );
    Ok(())
}
