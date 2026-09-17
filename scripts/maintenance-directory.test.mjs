import assert from "node:assert/strict";
import test from "node:test";

import { directoryBytes } from "./maintenance-resources.mjs";

function entry({ size = 30, directory = false, symbolic = false, ino = 3n } = {}) {
  return { dev: 1n, ino, size, isDirectory: () => directory, isSymbolicLink: () => symbolic };
}

const rootEntry = () => entry({ size: 100, directory: true, ino: 2n });
const missing = (code) => Object.assign(new Error(`observed ${code}`), { code });

test("a Chromium child removed after enumeration is explicitly gone, not an unreadable live entry", async () => {
  for (const code of ["ENOENT", "ESRCH"]) {
    const bytes = await directoryBytes("/scratch", 1000, {
      async lstat(path) {
        if (path === "/scratch") return rootEntry();
        if (path.endsWith("gone")) throw missing(code);
        return entry();
      },
      readdir: async () => ["gone", "live"],
    });
    assert.equal(bytes, 130);
  }
});

test("child removal during readdir retains already observed bytes conservatively", async () => {
  for (const code of ["ENOENT", "ESRCH"]) {
    const bytes = await directoryBytes("/scratch", 1000, {
      lstat: async (path) => path === "/scratch" ? rootEntry() : entry({ directory: true }),
      async readdir(path) {
        if (path === "/scratch") return ["gone"];
        throw missing(code);
      },
    });
    assert.equal(bytes, 130);
  }
});

test("a missing root or still-live permission and I/O failure is never treated as empty", async () => {
  for (const code of ["ENOENT", "ESRCH", "EACCES", "EPERM", "EIO", "ENOTDIR"]) {
    await assert.rejects(directoryBytes("/scratch", 1000, {
      lstat: async () => { throw missing(code); }, readdir: async () => [],
    }), (error) => error.code === code);
  }
  for (const code of ["EACCES", "EPERM", "EIO", "ENOTDIR"]) {
    await assert.rejects(directoryBytes("/scratch", 1000, {
      async lstat(path) {
        if (path === "/scratch") return rootEntry();
        throw missing(code);
      },
      readdir: async () => ["live"],
    }), (error) => error.code === code);
    await assert.rejects(directoryBytes("/scratch", 1000, {
      lstat: async (path) => path === "/scratch" ? rootEntry() : entry({ directory: true }),
      readdir: async (path) => {
        if (path === "/scratch") return ["live"];
        throw missing(code);
      },
    }), (error) => error.code === code);
  }
  await assert.rejects(directoryBytes("/scratch", 1000, {
    lstat: async () => rootEntry(), readdir: async () => { throw missing("ENOENT"); },
  }), (error) => error.code === "ENOENT");
});

test("malformed sizes, escaping names and an oversized observation inventory fail closed", async () => {
  for (const size of [-1, NaN, Infinity, "0"]) {
    await assert.rejects(directoryBytes("/scratch", 1000, {
      lstat: async () => entry({ size, directory: true }), readdir: async () => [],
    }), /size is unobservable/u);
  }
  for (const names of [[".."], ["../escape"], ["/escape"], [""], Array(10_000).fill("entry")]) {
    await assert.rejects(directoryBytes("/scratch", 1000, {
      lstat: async () => rootEntry(), readdir: async () => names,
    }), /noncanonical|observation ceiling/u);
  }
});

test("root and child-directory replacement cannot redirect enumeration", async () => {
  await assert.rejects(directoryBytes("/scratch", 1000, {
    lstat: async () => entry({ directory: true, symbolic: true }), readdir: async () => [],
  }), /non-symlink/u);
  await assert.rejects(directoryBytes("/scratch", 1000, {
    lstat: async () => rootEntry(), readdir: async () => [],
  }, "1:999"), /identity changed/u);
  let reads = 0;
  await assert.rejects(directoryBytes("/scratch", 1000, {
    lstat: async () => entry({ directory: true, ino: BigInt(++reads) }),
    readdir: async () => [],
  }), /changed during enumeration/u);
});
