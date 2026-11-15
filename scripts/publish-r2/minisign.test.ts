import assert from "node:assert/strict";
import {
  createHash,
  generateKeyPairSync,
  type KeyObject,
  sign,
} from "node:crypto";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { versionManifestKey } from "./manifest.ts";
import { verifyMinisign } from "./minisign.ts";
import { publishRelease, releaseAssetNames } from "./publish.ts";
import { directoryStore } from "./store.ts";

const wrap = (text: string) => Buffer.from(text).toString("base64");
const KEY_ID = Buffer.from("0123456789abcdef", "hex");

function keyPair(keyId = KEY_ID) {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const line = Buffer.concat([Buffer.from("Ed"), keyId, raw]).toString(
    "base64",
  );
  return {
    privateKey,
    keyId,
    publicKey: wrap(`untrusted comment: minisign public key\n${line}\n`),
  };
}

/** Writes what `tauri signer sign` writes: a base64-wrapped, prehashed minisign signature. */
function signFile(
  key: { privateKey: KeyObject; keyId: Buffer },
  bytes: Uint8Array,
  comment: string,
  algorithm = "ED",
) {
  const signature = sign(
    null,
    algorithm === "ED"
      ? createHash("blake2b512").update(bytes).digest()
      : bytes,
    key.privateKey,
  );
  const line = Buffer.concat([Buffer.from(algorithm), key.keyId, signature]);
  const global = sign(
    null,
    Buffer.concat([signature, Buffer.from(comment)]),
    key.privateKey,
  );
  return wrap(
    `untrusted comment: signature from tauri secret key\n${line.toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`,
  );
}

const payload = Buffer.from("an installer");
const comment = "timestamp:1\tfile:OpenRize_1.2.3_x64-setup.exe";
const rewrap = (signature: string, edit: (text: string) => string) =>
  wrap(edit(Buffer.from(signature, "base64").toString("utf8")));

test("a genuine signature verifies and returns its trusted comment", () => {
  const key = keyPair();
  const signature = signFile(key, payload, comment);
  assert.equal(verifyMinisign(payload, signature, key.publicKey), comment);
});

test("a tampered payload, wrong key or tampered trusted comment is rejected", () => {
  const key = keyPair();
  const signature = signFile(key, payload, comment);
  assert.throws(
    () =>
      verifyMinisign(Buffer.from("an installer!"), signature, key.publicKey),
    /does not match the file/,
  );
  assert.throws(
    () => verifyMinisign(payload, signature, keyPair().publicKey),
    /does not match the file/,
  );
  assert.throws(
    () =>
      verifyMinisign(
        payload,
        signature,
        keyPair(Buffer.from("fedcba9876543210", "hex")).publicKey,
      ),
    /different key/,
  );
  assert.throws(
    () =>
      verifyMinisign(
        payload,
        rewrap(signature, (text) => text.replace("1.2.3", "1.2.4")),
        key.publicKey,
      ),
    /trusted comment signature/,
  );
});

test("unprehashed and malformed signatures are rejected", () => {
  const key = keyPair();
  assert.throws(
    () =>
      verifyMinisign(
        payload,
        signFile(key, payload, comment, "Ed"),
        key.publicKey,
      ),
    /unsupported minisign signature algorithm/,
  );
  assert.throws(() => verifyMinisign(payload, "", key.publicKey), /malformed/);
  assert.throws(
    () => verifyMinisign(payload, signFile(key, payload, comment), ""),
    /malformed/,
  );
});

test("the Windows installer is published only with the updater key's signature for that version", async () => {
  const key = keyPair();
  const installer = Buffer.alloc(1_000_000, "MZ");
  const dir = mkdtempSync(join(tmpdir(), "openrize-publish-test-"));
  try {
    const path = join(dir, releaseAssetNames("1.2.3").windows);
    const publish = () =>
      publishRelease(directoryStore(join(dir, "bucket")), {
        version: "1.2.3",
        dir,
        publicKey: key.publicKey,
      });
    writeFileSync(path, installer);
    await assert.rejects(publish, /not signed by the updater key/);

    writeFileSync(`${path}.sig`, signFile(keyPair(), installer, comment));
    await assert.rejects(publish, /not signed by the updater key/);

    const older = "timestamp:1\tfile:OpenRize_1.2.2_x64-setup.exe";
    writeFileSync(`${path}.sig`, signFile(key, installer, older));
    await assert.rejects(publish, /made for another file/);

    writeFileSync(`${path}.sig`, signFile(key, installer, comment));
    await publish();
    assert.ok(
      await directoryStore(join(dir, "bucket")).get(
        versionManifestKey("1.2.3"),
      ),
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
