# Feature Study — File Transfer Over Interactive Sessions

> Not a study of a single protocol — a study of how the established
> approaches handle file transfer alongside interactive traffic, and
> what OpenRD should learn from each.

## Why a dedicated study

OpenRD's [`requirements`](../02-requirements.md) make file transfer a
first-class channel, peer to display and input rather than an
afterthought. Several use cases (UC-1: 600 MB log bundle; UC-2:
diagnostic tool + report exchange; UC-3: design mockups dropped onto
the remote machine) put it directly on the user's critical path.

Most remote-desktop protocols either skip file transfer entirely
(VNC, Sunshine/Moonlight) or bolt it on awkwardly (RDP's drive
redirection, vendor extensions of VNC). We need to do better. To do
better, we need to know what each approach gets right and wrong.

## Approaches surveyed

1. **RDP "drive redirection"** (MS-RDPEFS)
2. **VNC TightFileTransfer** and other VNC extensions
3. **SFTP** (SSH File Transfer Protocol)
4. **rsync**
5. **WebDAV**
6. **HTTP/2 and HTTP/3 multipart**
7. **BitTorrent** (for the resumability/chunking ideas, not for use)
8. **RustDesk file transfer**

---

### 1. RDP drive redirection (MS-RDPEFS)

RDP's approach is to **expose client filesystems as virtual drives on
the server**. The server sees `\\tsclient\C` as a network share; copying
to or from it generates `IRP_MJ_READ` and `IRP_MJ_WRITE` requests that
travel back to the client over the RDPDR channel.

**Pros:**

- Transparent to the user. Drag-and-drop "just works" via the OS file
  manager.
- Streaming — large files are read/written in 64 KB chunks; no
  buffering of the whole file.
- Full POSIX-ish semantics (open, read, write, seek, stat).

**Cons:**

- Per-IRP round trips can be very chatty over WAN. A `cp` of 100,000
  small files can take an hour over a 50 ms link because every file
  is multiple sync request/response pairs.
- Implementing the full IRP-over-channel surface is enormous.
- The client must run as a user who can open the underlying files,
  which complicates sandboxing on the client side.
- Not a "transfer" — it's a remote file system. You can never tell
  whether a file is "done."

### 2. VNC TightFileTransfer (and friends)

TightVNC's `TightFileTransfer` is a message set on top of the existing
RFB stream: `FileListRequest`, `FileDownloadRequest`, `FileUploadStart`,
chunked data, MD5 verification.

**Pros:**

- Simple, debuggable.
- Explicit operations — the user starts a transfer and it completes.

**Cons:**

- Lives in the single RFB TCP stream → head-of-line blocking against
  the display channel.
- No directory tree support in some implementations.
- No resumability.
- Every VNC fork did it differently → no interop.

### 3. SFTP (SSH File Transfer Protocol)

The reference for "well-designed file transfer over an interactive
session." SFTP is **not** FTP — it's a request/response protocol
running inside an SSH channel, with operations like
`SSH_FXP_OPEN`, `SSH_FXP_READ`, `SSH_FXP_WRITE`, `SSH_FXP_READDIR`,
`SSH_FXP_STAT`.

**Pros:**

- Mature, widely deployed, interoperable.
- Operates as a normal SSH channel, so it gets the SSH multiplexing
  and security for free.
- Resumable by virtue of explicit offset reads/writes.
- Handles directory trees, permissions, timestamps.

**Cons:**

- Strict request/response — high latency on small-file-heavy
  transfers without pipelining.
- The IETF draft never reached RFC status (frozen at draft-13 in 2006);
  some semantic edges are server-defined.
- Designed for terminal-oriented use, not for drag-and-drop UX.

### 4. rsync

rsync is the gold standard for *delta* transfer — sending only the
parts of a file that changed.

**Pros:**

- Bandwidth-optimal for incremental sync (the rolling-checksum
  algorithm is genuinely clever).
- Handles directory trees natively.

**Cons:**

- Not an interactive-session protocol — it runs as a separate process
  over an SSH transport or a daemon protocol on TCP/873.
- Resource-heavy on the sender (rolling checksum over the whole file).
- Overkill for "drag this file once."

The rsync algorithm is worth understanding even though we won't
implement it in v0 — for v1, a "send only the changed blocks" mode
would be a powerful feature.

### 5. WebDAV

HTTP-based file operations. Lives at the HTTP layer.

**Pros:**

- Reuses HTTP semantics (auth, caching, range requests).
- Browser-native via fetch / streams.

**Cons:**

- Designed for "file server" use, not "session companion."
- Range requests are great for resumability but the protocol is
  verbose; metadata operations are an XML acid trip.

### 6. HTTP/2 and HTTP/3 multipart

Not a file-transfer protocol per se, but the modern way to move bytes:
multiplexed streams, range support, content negotiation.

**Pros:**

- Existing libraries everywhere.
- HTTP/3 (QUIC) gets connection migration and head-of-line freedom
  for free.

**Cons:**

- Adds an HTTP layer on top of the bytes. For an in-session transfer
  this is unnecessary indirection.

### 7. BitTorrent (chunking lessons only)

We are not using BitTorrent. But the *chunking + per-chunk hash + Merkle
tree of the manifest* design is a clean way to:

- Verify a file's integrity progressively (after chunk *k* of *n*,
  you have proven the first *k* chunks).
- Resume a transfer from any chunk.
- Re-fetch only the chunks that failed verification.

OpenRD's file channel should use the same shape: a transfer is a
sequence of fixed-size chunks (or a single chunk for tiny files) with
explicit per-chunk integrity.

### 8. RustDesk file transfer

RustDesk's file transfer rides on the same Protobuf-multiplexed
connection as everything else. It is explicit (the user initiates),
chunked, with progress messages, but it competes with input/display
for the single transport.

**Pros:** explicit, debuggable, has a real UI.
**Cons:** single-stream → can affect interactive responsiveness.

---

## Design implications for OpenRD

Combining the lessons:

1. **A dedicated File channel running on its own QUIC stream.**
   Independent streams in QUIC mean a slow file transfer cannot
   block the Input or Display channels. This solves the head-of-line
   problem that haunts RDP/VNC/RustDesk.

2. **Explicit transfer operations, not a mounted filesystem.**
   Drive-redirection (RDP-style) is too complex for v0 and creates
   client-side sandboxing problems. v0: explicit "send this file" /
   "receive this file" operations. Drive-redirection-style behavior
   is a v2 feature *built on top of* the File channel.

3. **Chunk + hash structure** for every transfer:

   ```
   StartTransfer { transfer_id, total_size, chunk_size, n_chunks,
                   per_chunk_sha256[], root_sha256 }
   Chunk { transfer_id, chunk_index, bytes }
   AckChunk { transfer_id, chunk_index, ok | hash_mismatch }
   EndTransfer { transfer_id, status }
   ```

4. **Default chunk size 256 KB**, configurable from 4 KB to 4 MB.
   256 KB amortizes per-chunk overhead while staying small enough
   to allow reasonable resume granularity.

5. **Directory trees supported in v0** but as a stream of file
   transfers prefixed by a manifest, not as a recursive opcode.
   The manifest is just a JSON or CBOR list of paths + sizes +
   permissions.

6. **Resumability**: a transfer that drops can be resumed if the
   receiver still has the manifest and partial chunks; the sender
   replays only the missing or hash-mismatched chunks.

7. **Out-of-scope for v0**: delta sync (rsync-style), wire-level
   compression of file content, encrypted-at-rest staging, file
   change notifications.

## References

- MS-RDPEFS — Remote Desktop Protocol: File System Virtual Channel
  Extension
- draft-ietf-secsh-filexfer-13 — SSH File Transfer Protocol
- "The rsync algorithm", Tridgell & Mackerras, 1996
- RFC 4918 — HTTP Extensions for WebDAV
- BitTorrent v2 (BEP-52) — Merkle hash trees over file content
