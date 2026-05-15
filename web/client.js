// OpenRD web client — v0 scaffolding.
//
// Runs the same flow as openrd-test-client over WebTransport:
//   ClientHello → ServerHello → AuthRequest → AuthResult →
//   OpenChannel(Input) → OpenChannelAck → uni stream with a few events.

const logEl = document.getElementById('log');
function log(s, kind) {
  const t = new Date().toISOString().split('T')[1].replace('Z', '');
  const line = document.createElement('div');
  line.textContent = `[${t}] ${s}`;
  if (kind === 'good') line.style.color = '#7fdb7f';
  if (kind === 'bad')  line.style.color = '#ff8b8b';
  if (kind === 'sys')  line.style.color = '#888';
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

// Control frame types (must match openrd-proto::control::ControlFrameType).
const Control = {
  ClientHello:     0x01,
  ServerHello:     0x02,
  AuthRequest:     0x03,
  AuthResult:      0x05,
  OpenChannel:     0x06,
  OpenChannelAck:  0x07,
};
// Channel kinds and Input frame types.
const ChannelKind = { Control: 0x0001, Display: 0x0002, Input: 0x0004 };
const InputFrame  = { KeyEvent: 0x01, TextInput: 0x07 };

document.getElementById('go').addEventListener('click', async () => {
  const url  = document.getElementById('url').value;
  const hash = document.getElementById('hash').value;
  const pin  = document.getElementById('pin').value;

  let certHash;
  try {
    certHash = parseHash(hash);
  } catch (e) {
    log(`bad cert hash: ${e}`, 'bad');
    return;
  }
  if (!pin) {
    log('PIN required', 'bad');
    return;
  }

  log(`connecting to ${url}...`, 'sys');
  let wt;
  try {
    wt = new WebTransport(url, {
      serverCertificateHashes: [{ algorithm: 'sha-256', value: certHash }],
    });
    await wt.ready;
    log('WebTransport ready', 'good');
  } catch (e) {
    log(`connect failed: ${e}`, 'bad');
    return;
  }

  let stream;
  try {
    stream = await wt.createBidirectionalStream();
    log('opened Control bidirectional stream', 'good');
  } catch (e) {
    log(`stream open failed: ${e}`, 'bad');
    return;
  }

  const writer = stream.writable.getWriter();
  const reader = stream.readable.getReader();
  const framer = new FrameReader(reader);

  // --- ClientHello ----
  await writer.write(encodeFrame(Control.ClientHello, encodeClientHello()));
  log('sent ClientHello');

  const sh = await framer.next();
  if (!sh || sh.type !== Control.ServerHello) {
    log(`expected ServerHello, got ${sh ? `type=0x${sh.type.toString(16)}` : 'EOF'}`, 'bad');
    return;
  }
  describeServerHello(new CborReader(sh.payload).readValue());

  // --- AuthRequest ----
  await writer.write(encodeFrame(Control.AuthRequest, encodeAuthRequest('pin', new TextEncoder().encode(pin))));
  log('sent AuthRequest (method=pin)');

  const ar = await framer.next();
  if (!ar || ar.type !== Control.AuthResult) {
    log(`expected AuthResult, got ${ar ? `type=0x${ar.type.toString(16)}` : 'EOF'}`, 'bad');
    return;
  }
  const result = new CborReader(ar.payload).readValue();
  const status = (result.get(1) ?? 0) | 0;
  const permission = result.get(2) ?? '';
  const identity = result.get(3) ?? '';
  log(`AuthResult: status=${status} permission=${permission} identity=${identity}`,
      status === 0 ? 'good' : 'bad');
  if (status !== 0) return;

  // --- OpenChannel(Input) ----
  await writer.write(encodeFrame(Control.OpenChannel, encodeOpenChannel(ChannelKind.Input, 1, 2)));
  log('sent OpenChannel(Input, channel_id=1)');

  const ack = await framer.next();
  if (!ack || ack.type !== Control.OpenChannelAck) {
    log(`expected OpenChannelAck, got ${ack ? `type=0x${ack.type.toString(16)}` : 'EOF'}`, 'bad');
    return;
  }
  const ackVal = new CborReader(ack.payload).readValue();
  const ackStatus = (ackVal.get(2) ?? 0) | 0;
  log(`OpenChannelAck: status=${ackStatus}`, ackStatus === 0 ? 'good' : 'bad');
  if (ackStatus !== 0) return;

  // --- Open Input uni stream and push a few events ----
  const inputStream = await wt.createUnidirectionalStream();
  const inputWriter = inputStream.getWriter();
  log('opened Input unidirectional stream');

  await inputWriter.write(encodeFrame(InputFrame.KeyEvent, encodeKeyEvent({ keysym: 0x0061, scancode: 0x001E, modifiers: 0, flags: 0x01 })));
  log("sent KeyEvent ('a' down)");
  await inputWriter.write(encodeFrame(InputFrame.KeyEvent, encodeKeyEvent({ keysym: 0x0061, scancode: 0x001E, modifiers: 0, flags: 0x00 })));
  log("sent KeyEvent ('a' up)");
  await inputWriter.write(encodeFrame(InputFrame.TextInput, encodeTextInput('olá, mundo 🌍 (web)')));
  log("sent TextInput");

  await inputWriter.close();
  log('finished Input uni stream', 'good');

  // Give server a beat to drain, then close.
  await new Promise(r => setTimeout(r, 200));
  wt.close({ closeCode: 0, reason: 'bye' });
  log('done.', 'good');
});

function describeServerHello(map) {
  if (!(map instanceof Map)) { log('ServerHello not a map', 'bad'); return; }
  log('ServerHello:', 'good');
  log(`  protocol_version: ${map.get(1)}`);
  log(`  server_name:      ${JSON.stringify(map.get(2))}`);
  const caps = map.get(3);
  if (caps instanceof Map) {
    log(`  capabilities.profile:        ${caps.get(2)}`);
    log(`  capabilities.auth_methods:   ${JSON.stringify(caps.get(3))}`);
    log(`  capabilities.display_codecs: ${JSON.stringify(caps.get(4))}`);
  }
  const sid = map.get(4);
  if (sid instanceof Uint8Array) {
    log(`  session_id:       ${hex(sid)}`);
  }
  log(`  server_time:      ${map.get(5)}`);
}

function parseHash(s) {
  const hex = s.replace(/[\s:]/g, '');
  if (hex.length !== 64) throw new Error(`expected 64 hex chars (32 bytes), got ${hex.length}`);
  const out = new Uint8Array(32);
  for (let i = 0; i < 32; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}

function hex(bytes) {
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
}

// --- Frame envelope ----

function encodeFrame(frameType, payload) {
  const out = new Uint8Array(6 + payload.length);
  out[0] = 0x00;                                       // version
  out[1] = frameType;
  new DataView(out.buffer).setUint32(2, payload.length, true);  // little-endian
  out.set(payload, 6);
  return out;
}

class FrameReader {
  constructor(reader) { this.reader = reader; this.buf = new Uint8Array(0); }
  async next() {
    while (true) {
      if (this.buf.length >= 6) {
        const length = new DataView(this.buf.buffer, this.buf.byteOffset, this.buf.byteLength).getUint32(2, true);
        if (this.buf.length >= 6 + length) {
          const version = this.buf[0];
          const type    = this.buf[1];
          const payload = this.buf.slice(6, 6 + length);
          this.buf = this.buf.slice(6 + length);
          return { version, type, payload };
        }
      }
      const { value, done } = await this.reader.read();
      if (done) return null;
      const next = new Uint8Array(this.buf.length + value.length);
      next.set(this.buf, 0);
      next.set(value, this.buf.length);
      this.buf = next;
    }
  }
}

// --- Control payload encoders / decoders ----

function encodeClientHello() {
  const caps = encodeCapabilities();
  const w = new CborWriter();
  w.writeMapHeader(3);
  w.writeUint(1); w.writeUint(0);
  w.writeUint(2); w.writeString('openrd-web/0.0.1');
  w.writeUint(3); w.writeRaw(caps);
  return w.finish();
}

function encodeCapabilities() {
  // Mirror Capabilities::default() from openrd-proto for negotiation parity.
  const w = new CborWriter();
  w.writeMapHeader(15);
  w.writeUint(1);  w.writeArrayHeader(1); w.writeUint(0);
  w.writeUint(2);  w.writeString('openrd-v0-base');
  w.writeUint(3);  w.writeArrayHeader(2); w.writeString('pin'); w.writeString('token');
  w.writeUint(4);  w.writeArrayHeader(1); w.writeString('h264-baseline');
  w.writeUint(5);  w.writeArrayHeader(2); w.writeUint(1920); w.writeUint(1080);
  w.writeUint(6);  w.writeUint(30);
  w.writeUint(7);  w.writeArrayHeader(1); w.writeString('opus');
  w.writeUint(8);  w.writeArrayHeader(2); w.writeString('text/plain;charset=utf-8'); w.writeString('image/png');
  w.writeUint(9);  w.writeUint(4);
  w.writeUint(10); w.writeUint(17179869184);
  w.writeUint(11); w.writeArrayHeader(2); w.writeUint(4096); w.writeUint(4 * 1024 * 1024);
  w.writeUint(12); w.writeUint(64 * 1024 * 1024);
  w.writeUint(13); w.writeUint(30);
  w.writeUint(14); w.writeArrayHeader(1); w.writeString('quic-datagrams');
  w.writeUint(16); w.writeMapHeader(2); w.writeUint(1); w.writeBool(true); w.writeUint(2); w.writeUint(1024 * 1024);
  return w.finish();
}

function encodeAuthRequest(method, credentialBytes) {
  const w = new CborWriter();
  w.writeMapHeader(2);
  w.writeUint(1); w.writeString(method);
  w.writeUint(2); w.writeBytes(credentialBytes);
  return w.finish();
}

function encodeOpenChannel(kind, channelId, streamId) {
  const w = new CborWriter();
  w.writeMapHeader(4);
  w.writeUint(1); w.writeUint(kind);
  w.writeUint(2); w.writeUint(channelId);
  w.writeUint(3); w.writeUint(streamId);
  w.writeUint(4); w.writeMapHeader(0);
  return w.finish();
}

// --- Input payload encoders ----

function encodeKeyEvent({ keysym, scancode, modifiers, flags }) {
  const out = new Uint8Array(13);
  const dv = new DataView(out.buffer);
  dv.setUint32(0, keysym, true);
  dv.setUint32(4, scancode, true);
  dv.setUint32(8, modifiers, true);
  out[12] = flags;
  return out;
}

function encodeTextInput(text) {
  const bytes = new TextEncoder().encode(text);
  const out = new Uint8Array(4 + bytes.length);
  new DataView(out.buffer).setUint32(0, bytes.length, true);
  out.set(bytes, 4);
  return out;
}

// --- Minimal CBOR writer ----

class CborWriter {
  constructor() { this.chunks = []; }
  finish() {
    const total = this.chunks.reduce((n, c) => n + c.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of this.chunks) { out.set(c, off); off += c.length; }
    return out;
  }
  writeRaw(bytes)      { this.chunks.push(bytes); }
  writeUint(n)         { this._typedNum(0x00, n); }
  writeMapHeader(n)    { this._typedNum(0xA0, n); }
  writeArrayHeader(n)  { this._typedNum(0x80, n); }
  writeBytes(b)        { this._typedNum(0x40, b.length); this.chunks.push(b); }
  writeString(s) {
    const enc = new TextEncoder().encode(s);
    this._typedNum(0x60, enc.length);
    this.chunks.push(enc);
  }
  writeBool(b)         { this.chunks.push(new Uint8Array([b ? 0xF5 : 0xF4])); }
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
    } else if (n < 0x1_0000_0000) {
      const b = new Uint8Array(5);
      b[0] = majorBase | 26;
      new DataView(b.buffer).setUint32(1, n, false);
      this.chunks.push(b);
    } else {
      const b = new Uint8Array(9);
      b[0] = majorBase | 27;
      const dv = new DataView(b.buffer);
      const big = BigInt(n);
      dv.setUint32(1, Number(big >> 32n), false);
      dv.setUint32(5, Number(big & 0xFFFFFFFFn), false);
      this.chunks.push(b);
    }
  }
}

// --- Minimal CBOR reader ----

class CborReader {
  constructor(bytes) { this.bytes = bytes; this.pos = 0; }
  readValue() {
    if (this.pos >= this.bytes.length) throw new Error('CBOR: unexpected EOF');
    const initial = this.bytes[this.pos++];
    const major = initial >> 5;
    const info  = initial & 0x1f;
    const length = this._readLength(info);
    switch (major) {
      case 0: return Number(length);
      case 1: return -1 - Number(length);
      case 2: {
        const n = Number(length);
        const b = this.bytes.slice(this.pos, this.pos + n);
        this.pos += n;
        return b;
      }
      case 3: {
        const n = Number(length);
        const s = new TextDecoder().decode(this.bytes.subarray(this.pos, this.pos + n));
        this.pos += n;
        return s;
      }
      case 4: {
        const arr = [];
        for (let i = 0; i < Number(length); i++) arr.push(this.readValue());
        return arr;
      }
      case 5: {
        const m = new Map();
        for (let i = 0; i < Number(length); i++) {
          const k = this.readValue();
          const v = this.readValue();
          m.set(k, v);
        }
        return m;
      }
      case 7:
        if (info === 20) return false;
        if (info === 21) return true;
        if (info === 22) return null;
        if (info === 23) return undefined;
        throw new Error(`CBOR: unsupported simple ${info}`);
      default:
        throw new Error(`CBOR: unsupported major ${major}`);
    }
  }
  _readLength(info) {
    if (info < 24) return info;
    if (info === 24) return this.bytes[this.pos++];
    if (info === 25) {
      const v = (this.bytes[this.pos] << 8) | this.bytes[this.pos + 1];
      this.pos += 2; return v;
    }
    if (info === 26) {
      const v = this.bytes[this.pos] * 0x1000000
              + ((this.bytes[this.pos + 1] << 16) | (this.bytes[this.pos + 2] << 8) | this.bytes[this.pos + 3]);
      this.pos += 4; return v;
    }
    if (info === 27) {
      const dv = new DataView(this.bytes.buffer, this.bytes.byteOffset + this.pos, 8);
      const hi = BigInt(dv.getUint32(0, false));
      const lo = BigInt(dv.getUint32(4, false));
      this.pos += 8;
      return (hi << 32n) | lo;
    }
    throw new Error(`CBOR: unsupported length ${info}`);
  }
}
