// Makes sure libmpv is available before building or running the app.
// Windows: downloads the LGPL build into third_party/mpv/windows-x64 if missing.
// macOS/Linux: the system libmpv is used; we only check and explain.
import { existsSync } from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

if (process.env.ONESHOT_LIBMPV) {
  if (!existsSync(process.env.ONESHOT_LIBMPV)) {
    console.error(`ONESHOT_LIBMPV points to a missing file: ${process.env.ONESHOT_LIBMPV}`);
    process.exit(1);
  }
  process.exit(0);
}

if (process.platform === "win32") {
  const dll = join(root, "third_party", "mpv", "windows-x64", "libmpv-2.dll");
  if (existsSync(dll)) process.exit(0);
  console.log("libmpv not found: downloading the LGPL build (one time, ~30 MB compressed)…");
  const r = spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(root, "tools", "fetch-libmpv.ps1")], {
    stdio: "inherit",
  });
  process.exit(r.status ?? 1);
}

// macOS / Linux: libmpv comes from the system.
const probe = process.platform === "darwin" ? ["brew", ["--prefix", "mpv"]] : ["ldconfig", ["-p"]];
try {
  const out = execFileSync(probe[0], probe[1], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  if (process.platform === "darwin" || out.includes("libmpv.so")) process.exit(0);
} catch {
  /* fall through to the hint */
}
console.warn(
  process.platform === "darwin"
    ? "libmpv not found. Install it with `brew install mpv`, or set ONESHOT_LIBMPV to a libmpv.2.dylib."
    : "libmpv not found. Install your distribution's package (libmpv2 / mpv-libs), or set ONESHOT_LIBMPV.",
);
// Not fatal: the app starts and reports the missing engine in Settings > Advanced.
