// Makes sure libmpv is available before building or running the app.
// Windows: downloads the LGPL build into third_party/mpv/windows-x64 if missing.
// macOS (Apple silicon): bundles Homebrew's libmpv and its dependencies into third_party/mpv/macos-arm64.
// Linux: the system libmpv is used; we only check and explain.
// ffmpeg (converts titles for AirPlay) comes along: bundled on macOS and Windows, the system's on Linux.
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
  const fetch = (script) => spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(root, "tools", script)], { stdio: "inherit" }).status ?? 1;
  const dll = join(root, "third_party", "mpv", "windows-x64", "libmpv-2.dll");
  if (!existsSync(dll)) {
    console.log("libmpv not found: downloading the LGPL build (one time, ~30 MB compressed)…");
    const status = fetch("fetch-libmpv.ps1");
    if (status !== 0) process.exit(status);
  }
  // ffmpeg converts titles for AirPlay.
  if (!existsSync(join(root, "third_party", "ffmpeg", "windows-x64", "ffmpeg.exe"))) {
    console.log("ffmpeg not found: downloading the GPL build (one time)…");
    process.exit(fetch("fetch-ffmpeg.ps1"));
  }
  process.exit(0);
}

if (process.platform === "darwin") {
  if (process.arch !== "arm64") {
    console.error("Only Apple silicon builds are supported: libmpv cannot be bundled on this Mac.");
    process.exit(1);
  }
  const r = spawnSync(process.execPath, [join(root, "tools", "bundle-libmpv-macos.mjs")], { stdio: "inherit" });
  process.exit(r.status ?? 1);
}

// Linux: libmpv and ffmpeg (AirPlay conversions) come from the system.
if (spawnSync("ffmpeg", ["-version"], { stdio: "ignore" }).status !== 0) {
  console.warn("ffmpeg not found on the PATH: AirPlay cannot convert titles. Install your distribution's ffmpeg package, or set ONESHOT_FFMPEG.");
}
try {
  const out = execFileSync("ldconfig", ["-p"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  if (out.includes("libmpv.so")) process.exit(0);
} catch {
  /* fall through to the hint */
}
console.warn("libmpv not found. Install your distribution's package (libmpv2 / mpv-libs), or set ONESHOT_LIBMPV.");
// Not fatal: the app starts and reports the missing engine in Settings > Advanced.
