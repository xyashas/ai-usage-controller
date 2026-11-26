import { createCipheriv, createDecipheriv, createHash, createHmac, randomBytes, timingSafeEqual } from "node:crypto";

export const token = (bytes = 32) => randomBytes(bytes).toString("base64url");
export const id = () => randomBytes(12).toString("base64url");
export const hash = (s: string) => createHash("sha256").update(s).digest("hex");

/** Human-friendly invite code, e.g. "K7Q2-9XMD". No 0/O/1/I. */
export function inviteCode(): string {
  const alphabet = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
  const b = randomBytes(8);
  let s = "";
  for (let i = 0; i < 8; i++) s += alphabet[b[i] % alphabet.length];
  return `${s.slice(0, 4)}-${s.slice(4)}`;
}

function key(secret: string): Buffer {
  return createHash("sha256").update(secret).digest();
}

/** AES-256-GCM for secrets at rest (the Anthropic admin key). */
export function encrypt(plain: string, secret: string): string {
  const iv = randomBytes(12);
  const c = createCipheriv("aes-256-gcm", key(secret), iv);
  const body = Buffer.concat([c.update(plain, "utf8"), c.final()]);
  return [iv, c.getAuthTag(), body].map((b) => b.toString("base64url")).join(".");
}

export function decrypt(enc: string, secret: string): string {
  const [iv, tag, body] = enc.split(".").map((p) => Buffer.from(p, "base64url"));
  const d = createDecipheriv("aes-256-gcm", key(secret), iv);
  d.setAuthTag(tag);
  return Buffer.concat([d.update(body), d.final()]).toString("utf8");
}

/** Lemon Squeezy signs the raw body with HMAC-SHA256 (hex) in X-Signature. */
export function verifyLemonSqueezy(raw: string, signature: string | undefined, secret: string): boolean {
  if (!signature) return false;
  const expected = createHmac("sha256", secret).update(raw).digest("hex");
  const a = Buffer.from(expected, "utf8");
  const b = Buffer.from(signature, "utf8");
  return a.length === b.length && timingSafeEqual(a, b);
}
