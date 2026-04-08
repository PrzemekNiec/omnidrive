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

async function handleStreamDownload(shareId, dekBase64url, token) {
  try {
    const dekBytes = base64urlToBytes(dekBase64url);
    const cryptoKey = await crypto.subtle.importKey(
      'raw', dekBytes, { name: 'AES-GCM' }, false, ['decrypt']
    );

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

            const plaintext = await crypto.subtle.decrypt(
              { name: 'AES-GCM', iv: nonce, tagLength: 128 },
              cryptoKey,
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
