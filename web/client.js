// OpenRD web client v0 scaffolding.
//
// What this does today: opens a WebTransport connection, opens the
// Control bidirectional stream, sends a hand-rolled stub ClientHello
// frame, logs incoming bytes.
//
// What it doesn't do yet: parse ServerHello, run auth, open Display,
// decode H.264. All TODO.

const logEl = document.getElementById('log');
function log(s) {
  const t = new Date().toISOString().split('T')[1].replace('Z', '');
  logEl.textContent += `[${t}] ${s}\n`;
  logEl.scrollTop = logEl.scrollHeight;
}

document.getElementById('go').addEventListener('click', async () => {
  const url = document.getElementById('url').value;
  log(`connecting to ${url}...`);

  if (!('WebTransport' in window)) {
    log('error: WebTransport not supported in this browser.');
    log('try a recent Chrome/Edge build (Firefox is shipping support).');
    return;
  }

  let wt;
  try {
    wt = new WebTransport(url);
    await wt.ready;
    log('WebTransport ready');
  } catch (e) {
    log(`connect failed: ${e}`);
    return;
  }

  let stream;
  try {
    stream = await wt.createBidirectionalStream();
    log('opened Control bidirectional stream');
  } catch (e) {
    log(`stream open failed: ${e}`);
    return;
  }

  const writer = stream.writable.getWriter();
  const reader = stream.readable.getReader();

  // Build and send ClientHello (Control frame type 0x01).
  const payload = encodeClientHello({
    protocolVersion: 0,
    clientName: 'openrd-web/0.0.1',
    capabilities: {},  // empty for now
  });

  const frame = encodeFrame(0x01, payload);
  await writer.write(frame);
  log(`sent ClientHello (frame ${frame.length} B, payload ${payload.length} B)`);

  // Drain whatever the server emits back.
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        log('server stream ended');
        break;
      }
      log(`recv ${value.length} B: ${hex(value.slice(0, 32))}${value.length > 32 ? '...' : ''}`);
    }
  } catch (e) {
    log(`read error: ${e}`);
  }
});

function encodeFrame(frameType, payload) {
  const out = new Uint8Array(6 + payload.length);
  out[0] = 0x00;                                       // version
  out[1] = frameType;                                  // type
  const dv = new DataView(out.buffer);
  dv.setUint32(2, payload.length, true);               // length, little-endian
  out.set(payload, 6);
  return out;
}

function encodeClientHello({ protocolVersion, clientName, capabilities }) {
  // CBOR map with three keys: 1 -> protocol_version, 2 -> client_name,
  // 3 -> capabilities. Uses the minimal hand-rolled CBOR helper below
  // so we don't pull in a dependency for the scaffold.
  const w = new CborWriter();
  w.writeMapHeader(3);
  w.writeUint(1); w.writeUint(protocolVersion);
  w.writeUint(2); w.writeString(clientName);
  w.writeUint(3); w.writeMap(capabilities);
  return w.finish();
}

function hex(bytes) {
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join(' ');
}

// Minimal CBOR writer. Implements only the subset we need:
// uint, neg uint, byte string, text string, array header, map header.
class CborWriter {
  constructor() { this.chunks = []; }
  finish() {
    const total = this.chunks.reduce((n, c) => n + c.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of this.chunks) { out.set(c, off); off += c.length; }
    return out;
  }
  writeUint(n)         { this._typedNum(0x00, n); }
  writeMapHeader(n)    { this._typedNum(0xA0, n); }
  writeArrayHeader(n)  { this._typedNum(0x80, n); }
  writeBytes(b)        { this._typedNum(0x40, b.length); this.chunks.push(b); }
  writeString(s) {
    const enc = new TextEncoder().encode(s);
    this._typedNum(0x60, enc.length);
    this.chunks.push(enc);
  }
  writeMap(obj) {
    const entries = Object.entries(obj);
    this.writeMapHeader(entries.length);
    for (const [k, v] of entries) {
      this.writeString(k);
      this._writeAny(v);
    }
  }
  _writeAny(v) {
    if (typeof v === 'number' && Number.isInteger(v) && v >= 0) this.writeUint(v);
    else if (typeof v === 'string') this.writeString(v);
    else if (v instanceof Uint8Array) this.writeBytes(v);
    else if (Array.isArray(v)) {
      this.writeArrayHeader(v.length);
      for (const x of v) this._writeAny(x);
    } else if (v && typeof v === 'object') {
      this.writeMap(v);
    } else {
      throw new Error(`CborWriter: unsupported value ${typeof v}`);
    }
  }
  _typedNum(majorBase, n) {
    if (n < 24) {
      this.chunks.push(new Uint8Array([majorBase | n]));
    } else if (n < 256) {
      this.chunks.push(new Uint8Array([majorBase | 24, n]));
    } else if (n < 65536) {
      const b = new Uint8Array(3);
      b[0] = majorBase | 25;
      new DataView(b.buffer).setUint16(1, n, false);
      this.chunks.push(b);
    } else {
      const b = new Uint8Array(5);
      b[0] = majorBase | 26;
      new DataView(b.buffer).setUint32(1, n, false);
      this.chunks.push(b);
    }
  }
}
