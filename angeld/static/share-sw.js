// OmniDrive Share Service Worker — streaming decryption for large files.
// Intercepts /sw-download/{share_id} requests. The page sends the DEK
// and access token via MessageChannel before navigating.
'use strict';

const PENDING = new Map(); // shareId -> { dek, token, resolve }

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

// Receive DEK + token from the page via MessageChannel
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'prepare-download') {
    const { shareId, dekBase64url, token } = event.data;
    PENDING.set(shareId, { dekBase64url, token });
    // Respond on the MessageChannel port to signal readiness
    if (event.ports && event.ports[0]) {
      event.ports[0].postMessage({ ready: true });
    }
  }
});

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);
  const match = url.pathname.match(/^\/sw-download\/(.+)$/);
  if (!match) return;

  const shareId = match[1];
  const pending = PENDING.get(shareId);
  if (!pending) {
    event.respondWith(new Response('Brak klucza deszyfrujacego w Service Worker.', { status: 400 }));
    return;
  }

  PENDING.delete(shareId);
  event.respondWith(handleStreamDownload(shareId, pending.dekBase64url, pending.token));
});

function base64urlToBytes(b64) {
  const b64std = b64.replace(/-/g, '+').replace(/_/g, '/');
  const pad = (4 - b64std.length % 4) % 4;
  const raw = atob(b64std + '='.repeat(pad));
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

// Musi zgadzac sie co do bajtu z deriveShareWrappingKey w share.html oraz z
// derive_subkey w Rust (HKDF-Expand bez extract = jeden blok HMAC dla 32 bajtow).
async function deriveShareWrappingKey(shareKeyBytes) {
  const info = new TextEncoder().encode('omnidrive-share-dek-v1');
  const block = new Uint8Array(info.length + 1);
  block.set(info, 0);
  block[info.length] = 0x01;

  const hmacKey = await crypto.subtle.importKey(
    'raw', shareKeyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']
  );
  const derived = await crypto.subtle.sign('HMAC', hmacKey, block);
  return crypto.subtle.importKey(
    'raw', new Uint8Array(derived), { name: 'AES-GCM' }, false, ['decrypt']
  );
}

async function openSealedDek(wrappingKey, packId, sealedBase64url) {
  const sealed = base64urlToBytes(sealedBase64url);
  const raw = await crypto.subtle.decrypt(
    {
      name: 'AES-GCM',
      iv: sealed.slice(0, 12),
      tagLength: 128,
      additionalData: new TextEncoder().encode(packId)
    },
    wrappingKey,
    sealed.slice(12)
  );
  return crypto.subtle.importKey(
    'raw', new Uint8Array(raw), { name: 'AES-GCM' }, false, ['decrypt']
  );
}

async function handleStreamDownload(shareId, dekBase64url, token) {
  try {
    const shareKeyBytes = base64urlToBytes(dekBase64url);
    const cryptoKey = await deriveShareWrappingKey(shareKeyBytes);

    const tokenParam = token ? '?token=' + encodeURIComponent(token) : '';
    const metaResp = await fetch('/api/share/' + shareId + '/meta' + tokenParam);
    if (!metaResp.ok) {
      return new Response('Blad pobierania metadanych pliku.', { status: metaResp.status });
    }

    const meta = await metaResp.json();
    const totalChunks = meta.chunks.length;

    const stream = new ReadableStream({
      async start(controller) {
        try {
          for (let i = 0; i < totalChunks; i++) {
            const chunkResp = await fetch(
              '/api/share/' + shareId + '/chunks/' + meta.chunks[i].index + tokenParam
            );
            if (!chunkResp.ok) {
              controller.error(new Error('Blad pobierania fragmentu ' + (i + 1)));
              return;
            }

            const encryptedBuf = await chunkResp.arrayBuffer();
            const encryptedArr = new Uint8Array(encryptedBuf);

            // nonce (12) || ciphertext + tag (rest)
            const nonce = encryptedArr.slice(0, 12);
            const ciphertextWithTag = encryptedArr.slice(12);

            const chunkKey = await openSealedDek(
              cryptoKey, meta.chunks[i].pack_id, meta.chunks[i].sealed_dek
            );
            const plaintext = await crypto.subtle.decrypt(
              { name: 'AES-GCM', iv: nonce, tagLength: 128 },
              chunkKey,
              ciphertextWithTag
            );

            controller.enqueue(new Uint8Array(plaintext));
          }
          controller.close();
        } catch (err) {
          controller.error(err);
        }
      }
    });

    return new Response(stream, {
      status: 200,
      headers: {
        'Content-Type': 'application/octet-stream',
        'Content-Disposition': 'attachment; filename="' + meta.file_name.replace(/"/g, '\\"') + '"',
        'Content-Length': String(meta.file_size),
      }
    });
  } catch (err) {
    return new Response('Blad deszyfrowania: ' + err.message, { status: 500 });
  }
}
