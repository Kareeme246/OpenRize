import { createHash, createPublicKey, verify } from "node:crypto";

const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

/** Both Tauri's public key and its `.sig` files are base64-wrapped minisign text files. */
const unwrap = (text: string) =>
  Buffer.from(text.trim(), "base64")
    .toString("utf8")
    .split("\n")
    .map((line) => line.replace(/\r$/, ""));

function field(line: string | undefined, length: number, what: string) {
  const bytes = Buffer.from(line ?? "", "base64");
  if (bytes.length !== length) throw new Error(`malformed minisign ${what}`);
  return bytes;
}

/**
 * Verifies a Tauri updater signature over `bytes` and returns its trusted comment, which the
 * signature also covers. `publicKey` is the updater `pubkey` from tauri.conf.json. Only minisign's
 * prehashed form (`ED`, BLAKE2b-512) is accepted: it is the one Tauri writes.
 */
export function verifyMinisign(
  bytes: Uint8Array,
  signature: string,
  publicKey: string,
): string {
  const key = field(unwrap(publicKey)[1], 42, "public key");
  const [, signatureLine, commentLine, globalLine] = unwrap(signature);
  const signed = field(signatureLine, 74, "signature");
  const globalSignature = field(globalLine, 64, "global signature");
  const prefix = "trusted comment: ";
  if (!commentLine?.startsWith(prefix)) {
    throw new Error("malformed minisign trusted comment");
  }
  const comment = commentLine.slice(prefix.length);

  if (key.subarray(0, 2).toString("latin1") !== "Ed") {
    throw new Error("unsupported minisign public key algorithm");
  }
  if (signed.subarray(0, 2).toString("latin1") !== "ED") {
    throw new Error("unsupported minisign signature algorithm");
  }
  if (!signed.subarray(2, 10).equals(key.subarray(2, 10))) {
    throw new Error("signed by a different key");
  }
  const ed25519 = createPublicKey({
    key: Buffer.concat([ED25519_SPKI_PREFIX, key.subarray(10)]),
    format: "der",
    type: "spki",
  });
  const signatureBytes = signed.subarray(10);
  const digest = createHash("blake2b512").update(bytes).digest();
  if (!verify(null, digest, ed25519, signatureBytes)) {
    throw new Error("signature does not match the file");
  }
  const global = Buffer.concat([signatureBytes, Buffer.from(comment, "utf8")]);
  if (!verify(null, global, ed25519, globalSignature)) {
    throw new Error("trusted comment signature does not match");
  }
  return comment;
}
