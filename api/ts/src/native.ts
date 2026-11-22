import koffi from "koffi";
import fs from "node:fs";
import path from "node:path";
import { EXPECTED_ABI_VERSION, SeleniumErrorKind, declare } from "./generated/native.g.js";

export class SeleniumError extends Error {
  constructor(
    readonly kind: SeleniumErrorKind,
    message: string,
  ) {
    super(`${SeleniumErrorKind[kind] ?? kind}: ${message}`);
    this.name = "SeleniumError";
  }
}

function libraryFile(): string {
  switch (process.platform) {
    case "win32":
      return "selenium_next_core.dll";
    case "darwin":
      return "libselenium_next_core.dylib";
    default:
      return "libselenium_next_core.so";
  }
}

/** Search order: SELENIUM_NEXT_NATIVE_DIR, <package>/native/<platform>-<arch>, <package>/native, repo build output. */
function locateLibrary(): string {
  const pkg = path.resolve(import.meta.dirname, "..");
  const file = libraryFile();
  const dirs = [
    process.env.SELENIUM_NEXT_NATIVE_DIR,
    path.join(pkg, "native", `${process.platform}-${process.arch}`),
    path.join(pkg, "native"),
    path.resolve(pkg, "..", "..", "core", "selenium-ffi", "target", "debug"),
  ];
  for (const dir of dirs) {
    if (!dir) continue;
    const candidate = path.join(dir, file);
    if (fs.existsSync(candidate)) return candidate;
  }
  throw new Error(`Native library ${file} not found; set SELENIUM_NEXT_NATIVE_DIR.`);
}

export const native = declare(koffi.load(locateLibrary()));

const actualAbi = native.se_abi_version() as number;
if (actualAbi !== EXPECTED_ABI_VERSION) {
  throw new Error(`Native library ABI version ${actualAbi} does not match bindings (${EXPECTED_ABI_VERSION}).`);
}

/** Throws SeleniumError when a native call reports failure. */
export function check(status: number): void {
  if (status === 0) return;
  throw new SeleniumError(status as SeleniumErrorKind, (native.se_last_error() as string) ?? "unknown native error");
}

const registry = new FinalizationRegistry<{ ptr: unknown; release: (p: unknown) => void }>((h) => h.release(h.ptr));

/** Owns one native reference; releases it on free() or when garbage collected. Never closes resources. */
export class NativeRef {
  private ptr: unknown;
  private readonly releaseFn: (p: unknown) => void;
  private done = false;

  constructor(ptr: unknown, release: (p: unknown) => void) {
    this.ptr = ptr;
    this.releaseFn = release;
    registry.register(this, { ptr, release }, this);
  }

  get released(): boolean {
    return this.done;
  }

  get raw(): unknown {
    if (this.done) throw new Error("Object has been disposed.");
    return this.ptr;
  }

  free(): void {
    if (this.done) return;
    this.done = true;
    registry.unregister(this);
    this.releaseFn(this.ptr);
  }
}
