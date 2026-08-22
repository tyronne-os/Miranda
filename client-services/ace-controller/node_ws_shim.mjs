/**
 * Minimal WebSocket server using only Node built-ins (no ws package required).
 * Implements enough of RFC6455 for JSON text frames used by EVE ECC.
 */
import { createHash, randomBytes } from "node:crypto";
import { EventEmitter } from "node:events";

const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

function acceptKey(key) {
  return createHash("sha1").update(key + GUID).digest("base64");
}

function encodeText(str) {
  const payload = Buffer.from(str, "utf8");
  const len = payload.length;
  let header;
  if (len < 126) {
    header = Buffer.alloc(2);
    header[0] = 0x81;
    header[1] = len;
  } else if (len < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x81;
    header[1] = 126;
    header.writeUInt16BE(len, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x81;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(len), 2);
  }
  return Buffer.concat([header, payload]);
}

function decodeFrames(buffer) {
  const messages = [];
  let offset = 0;
  while (offset + 2 <= buffer.length) {
    const b0 = buffer[offset];
    const b1 = buffer[offset + 1];
    const opcode = b0 & 0x0f;
    const masked = (b1 & 0x80) !== 0;
    let len = b1 & 0x7f;
    let pos = offset + 2;
    if (len === 126) {
      if (pos + 2 > buffer.length) break;
      len = buffer.readUInt16BE(pos);
      pos += 2;
    } else if (len === 127) {
      if (pos + 8 > buffer.length) break;
      len = Number(buffer.readBigUInt64BE(pos));
      pos += 8;
    }
    const maskLen = masked ? 4 : 0;
    if (pos + maskLen + len > buffer.length) break;
    let payload = buffer.subarray(pos + maskLen, pos + maskLen + len);
    if (masked) {
      const mask = buffer.subarray(pos, pos + 4);
      payload = Buffer.from(payload.map((b, i) => b ^ mask[i % 4]));
    }
    offset = pos + maskLen + len;
    if (opcode === 0x8) {
      messages.push({ type: "close" });
    } else if (opcode === 0x9) {
      messages.push({ type: "ping", data: payload });
    } else if (opcode === 0x1) {
      messages.push({ type: "text", data: payload.toString("utf8") });
    }
  }
  return { messages, rest: buffer.subarray(offset) };
}

export class WebSocket extends EventEmitter {
  constructor(socket) {
    super();
    this.socket = socket;
    this.readyState = 1; // OPEN
    this._buf = Buffer.alloc(0);
    socket.on("data", (chunk) => {
      this._buf = Buffer.concat([this._buf, chunk]);
      const { messages, rest } = decodeFrames(this._buf);
      this._buf = rest;
      for (const m of messages) {
        if (m.type === "text") this.emit("message", m.data);
        if (m.type === "close") {
          this.readyState = 3;
          this.emit("close");
          socket.end();
        }
        if (m.type === "ping") {
          // pong
          const pong = Buffer.concat([Buffer.from([0x8a, m.data.length]), m.data]);
          socket.write(pong);
        }
      }
    });
    socket.on("close", () => {
      this.readyState = 3;
      this.emit("close");
    });
    socket.on("error", (err) => this.emit("error", err));
  }

  send(data) {
    if (this.readyState !== 1) return;
    this.socket.write(encodeText(String(data)));
  }

  close() {
    this.readyState = 3;
    try {
      this.socket.end();
    } catch {
      /* ignore */
    }
  }
}

export class WebSocketServer extends EventEmitter {
  constructor({ server, path = "/ws" }) {
    super();
    this.path = path;
    server.on("upgrade", (req, socket, head) => {
      const url = new URL(req.url || "/", "http://localhost");
      if (url.pathname !== this.path) {
        socket.destroy();
        return;
      }
      const key = req.headers["sec-websocket-key"];
      if (!key) {
        socket.destroy();
        return;
      }
      const headers = [
        "HTTP/1.1 101 Switching Protocols",
        "Upgrade: websocket",
        "Connection: Upgrade",
        `Sec-WebSocket-Accept: ${acceptKey(key)}`,
        "\r\n",
      ].join("\r\n");
      socket.write(headers);
      if (head?.length) socket.unshift(head);
      const ws = new WebSocket(socket);
      this.emit("connection", ws, req);
    });
  }
}

export function randomId() {
  return randomBytes(6).toString("hex");
}
