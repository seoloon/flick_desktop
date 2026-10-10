// Gathers Homebrew's libmpv, and ffmpeg (for AirPlay conversions), with every non-system dylib they
// depend on, into two self-contained folders: each library's install name and dependencies point
// at @loader_path.
//   third_party/mpv/macos-arm64     libmpv.2.dylib          needs `brew install mpv`
//   third_party/ffmpeg/macos-arm64  ffmpeg                  needs `brew install ffmpeg-full`
// ffmpeg-full (not `ffmpeg`) because only it has libass, which burns text subtitles into the picture.
// It has libav* of its own, so it cannot share libmpv's folder. Apple silicon only.
import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readdirSync, realpathSync, rmSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const mpvDest = join(root, "third_party", "mpv", "macos-arm64");
const ffmpegDest = join(root, "third_party", "ffmpeg", "macos-arm64");

if (process.platform !== "darwin" || process.arch !== "arm64") {
  console.error("bundle-libmpv-macos: run this on an Apple silicon Mac.");
  process.exit(1);
}
if (existsSync(join(mpvDest, "libmpv.2.dylib")) && existsSync(join(ffmpegDest, "ffmpeg")) && !process.argv.includes("--force")) process.exit(0);

const run = (cmd, args) => execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const isSystem = (p) => p.startsWith("/usr/lib/") || p.startsWith("/System/");

function brewPrefix(formula, hint) {
  try {
    return run("brew", ["--prefix", formula]).trim();
  } catch {
    console.error(`${formula} not found. Install it with \`${hint}\`.`);
    process.exit(1);
  }
}

/** Install names a binary depends on, minus its own id. */
function deps(file) {
  const lines = run("otool", ["-L", file]).split("\n").slice(1).map((l) => l.trim().split(" (")[0]).filter(Boolean);
  return lines.filter((l) => l !== file && !isSystem(l));
}

/** Copies `entry` (saved as `name`) and its dependencies into `dest`, relocated and re-signed. Returns the file count. */
function bundle(dest, entry, name, libDir) {
  /** The real file behind one install name, as seen from the library that references it. */
  const resolve = (install, from) => {
    if (install.startsWith("@rpath/") || install.startsWith("@loader_path/") || install.startsWith("@executable_path/")) {
      const base = basename(install);
      for (const d of [dirname(from), libDir, "/opt/homebrew/lib"]) if (existsSync(join(d, base))) return realpathSync(join(d, base));
      throw new Error(`cannot resolve ${install} (needed by ${from})`);
    }
    return realpathSync(install);
  };

  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });

  // Breadth-first copy: original path -> file name in `dest`.
  const copied = new Map();
  const queue = [entry];
  const names = new Map([[entry, name]]);
  while (queue.length) {
    const file = queue.shift();
    if (copied.has(file)) continue;
    const out = names.get(file) ?? basename(file);
    copyFileSync(file, join(dest, out));
    chmodSync(join(dest, out), 0o644);
    copied.set(file, out);
    for (const d of deps(file)) {
      const real = resolve(d, file);
      if (!copied.has(real)) queue.push(real);
    }
  }
  if (name === "ffmpeg") chmodSync(join(dest, name), 0o755);

  // Rewrite every reference, then re-sign (editing invalidates the signature,
  // and arm64 refuses to load unsigned code).
  for (const [file, out] of copied) {
    const target = join(dest, out);
    run("install_name_tool", ["-id", `@loader_path/${out}`, target]);
    for (const d of deps(file)) {
      run("install_name_tool", ["-change", d, `@loader_path/${copied.get(resolve(d, file))}`, target]);
    }
    run("codesign", ["--force", "--sign", "-", target]);
  }

  // Nothing may still point outside the folder.
  for (const f of readdirSync(dest)) {
    const stray = run("otool", ["-L", join(dest, f)])
      .split("\n").slice(1).map((l) => l.trim().split(" (")[0])
      .filter((l) => l && !isSystem(l) && !l.startsWith("@loader_path/"));
    if (stray.length) {
      console.error(`${f} still references: ${stray.join(", ")}`);
      process.exit(1);
    }
  }
  return copied.size;
}

const mpv = brewPrefix("mpv", "brew install mpv");
const mpvFiles = bundle(mpvDest, realpathSync(join(mpv, "lib", "libmpv.2.dylib")), "libmpv.2.dylib", join(mpv, "lib"));
console.log(`libmpv bundled: ${mpvFiles} libraries in third_party/mpv/macos-arm64`);

const full = brewPrefix("ffmpeg-full", "brew install ffmpeg-full");
const ffmpegFiles = bundle(ffmpegDest, realpathSync(join(full, "bin", "ffmpeg")), "ffmpeg", join(full, "lib"));
console.log(`ffmpeg bundled: ${ffmpegFiles} files in third_party/ffmpeg/macos-arm64`);

// Text subtitles are burnt in with the `subtitles` filter (libass): check this build has it.
if (!run(join(ffmpegDest, "ffmpeg"), ["-hide_banner", "-filters"]).includes(" subtitles ")) {
  console.warn("warning: this ffmpeg has no `subtitles` filter (no libass): casting a title with a text subtitle to AirPlay will go without it.");
}
