import assert from "node:assert/strict";
import test from "node:test";

import { IDENTITY_MAX_BYTES, RUST_IDENTITY, rustIdentity } from "./maintenance-rust-identity.mjs";

const PATH = "/opt/bureau/rust/bin/rustup";
const METADATA = { uid: 0n, dev: 9007199254740993n, ino: 9007199254740995n, ctimeNs: 1234567890123456789n };
const host = async () => Buffer.from("         0          0 4294967295\n");

async function admitted() {
  const identity = await rustIdentity({}, true, host);
  identity.observe(PATH, METADATA);
  return identity.finish();
}

test("identity receipts preserve exact descriptor integers across the unmapped owner view", async () => {
  const receipt = await admitted();
  assert.deepEqual(JSON.parse(receipt).entries,
    [[PATH, "9007199254740993", "9007199254740995", "1234567890123456789"]]);
  const identity = await rustIdentity({ [RUST_IDENTITY]: receipt }, false);
  identity.observe(PATH, { ...METADATA, uid: 65534n });
  assert.equal(identity.finish(), receipt);
});

test("only an authoritative host-root observation may issue an admission receipt", async () => {
  for (const mapping of ["", "0 1000 1\n", "0 0 1\n", "invalid\n"]) {
    await assert.rejects(rustIdentity({}, true, async () => Buffer.from(mapping)), /remapped user namespace/u);
  }
  for (const uid of [1000n, 65534n, 0]) {
    const identity = await rustIdentity({}, true, host);
    assert.throws(() => identity.observe(PATH, { ...METADATA, uid }), /host-root ownership/u);
  }
});

test("rootless and user-remapped container startups cannot issue host-root evidence", async () => {
  for (const mapping of ["0 100000 65536\n", "0 1000 1\n1 231072 65535\n"]) {
    await assert.rejects(rustIdentity({}, true, async () => Buffer.from(mapping)),
      /outside a remapped user namespace/u);
  }
});

test("missing, malformed, oversized, duplicated and noncanonical receipts fail closed", async () => {
  const valid = JSON.parse(await admitted());
  const invalid = [
    undefined, "", "{", "x".repeat(IDENTITY_MAX_BYTES + 1),
    JSON.stringify({ ...valid, extra: true }), JSON.stringify({ ...valid, schema: "another" }),
    JSON.stringify({ ...valid, entries: [] }), JSON.stringify({ ...valid, entries: [...valid.entries, ...valid.entries] }),
    JSON.stringify({ ...valid, entries: Array.from({ length: 129 }, () => valid.entries[0]) }),
  ];
  for (const path of ["relative", "/opt/../bin", "/path with spaces", "/path\0", "/bad:entry"]) {
    invalid.push(JSON.stringify({ ...valid, entries: [[path, ...valid.entries[0].slice(1)]] }));
  }
  for (const field of [9007199254740993, "01", "-1", "1e10", null]) {
    invalid.push(JSON.stringify({ ...valid, entries: [[PATH, field, "1", "2"]] }));
  }
  for (const receipt of invalid) await assert.rejects(rustIdentity({ [RUST_IDENTITY]: receipt }, false));
});

test("each exact device, inode, ctime and path must match even when Number would round", async () => {
  const receipt = await admitted();
  assert.equal(Number(METADATA.dev), Number(METADATA.dev - 1n));
  for (const key of ["dev", "ino", "ctimeNs"]) {
    const identity = await rustIdentity({ [RUST_IDENTITY]: receipt }, false);
    assert.throws(() => identity.observe(PATH, { ...METADATA, [key]: METADATA[key] - 1n, uid: 65534n }),
      /identity receipt differs/u);
  }
  const identity = await rustIdentity({ [RUST_IDENTITY]: receipt }, false);
  assert.throws(() => identity.observe("/another/runtime", METADATA), /identity receipt differs/u);
  assert.throws(() => identity.finish(), /missing or unexpected/u);
});

test("incoherent startup observations and imprecise metadata cannot become a receipt", async () => {
  const identity = await rustIdentity({}, true, host);
  identity.observe(PATH, METADATA);
  assert.throws(() => identity.observe(PATH, { ...METADATA, ctimeNs: METADATA.ctimeNs + 1n }),
    /changed during admission/u);
  for (const key of ["dev", "ino", "ctimeNs"]) {
    for (const value of [undefined, 1, -1n]) {
      assert.throws(() => identity.observe(PATH, { ...METADATA, [key]: value }), /unobservable/u);
    }
  }
});
