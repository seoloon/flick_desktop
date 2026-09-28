// Gathers Homebrew's libmpv and every non-system dylib it depends on into
// third_party/mpv/macos-arm64, relocated so the folder is self-contained:
// each library's install name and dependencies point at @loader_path.
// Apple silicon only; needs `brew install mpv`.
import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readdirSync, realpathSync, rmSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dest = join(root, "third_party", "mpv", "macos-arm64");
const entry = join(dest, "libmpv.2.dylib");

if (process.platform !== "darwin" || process.arch !== "arm64") {
  console.error("bundle-libmpv-macos: run this on an Apple silicon Mac.");
  process.exit(1);
}
if (existsSync(entry) && !process.argv.includes("--force")) process.exit(0);

const run = (cmd, args) => execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const isSystem = (p) => p.startsWith("/usr/lib/") || p.startsWith("/System/");

let prefix;
try {
  prefix = run("brew", ["--prefix", "mpv"]).trim();
} catch {
  console.error("libmpv not found. Install it with `brew install mpv`.");
  process.exit(1);
}
const source = realpathSync(join(prefix, "lib", "libmpv.2.dylib"));

/** Install names a binary depends on, minus its own id. */
function deps(file) {
  const lines = run("otool", ["-L", file]).split("\n").slice(1).map((l) => l.trim().split(" (")[0]).filter(Boolean);
  return lines.filter((l) => l !== file && !isSystem(l));
}

/** The real file behind one install name, as seen from the library that references it. */
function resolve(name, from) {
  if (name.startsWith("@rpath/") || name.startsWith("@loader_path/") || name.startsWith("@executable_path/")) {
    const base = basename(name);
    const dirs = [dirname(from), join(prefix, "lib"), "/opt/homebrew/lib"];
    for (const d of dirs) if (existsSync(join(d, base))) return realpathSync(join(d, base));
    throw new Error(`cannot resolve ${name} (needed by ${from})`);
  }
  return realpathSync(name);
}

rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });

// Breadth-first copy: original path -> file name in `dest`.
const copied = new Map();
const queue = [source];
const names = new Map([[source, "libmpv.2.dylib"]]);
while (queue.length) {
  const file = queue.shift();
  if (copied.has(file)) continue;
  const name = names.get(file) ?? basename(file);
  copyFileSync(file, join(dest, name));
  chmodSync(join(dest, name), 0o644);
  copied.set(file, name);
  for (const d of deps(file)) {
    const real = resolve(d, file);
    if (!copied.has(real)) queue.push(real);
  }
}

// Rewrite every reference, then re-sign (editing invalidates the signature,
// and arm64 refuses to load unsigned code).
for (const [file, name] of copied) {
  const out = join(dest, name);
  run("install_name_tool", ["-id", `@loader_path/${name}`, out]);
  for (const d of deps(file)) {
    run("install_name_tool", ["-change", d, `@loader_path/${copied.get(resolve(d, file))}`, out]);
  }
  run("codesign", ["--force", "--sign", "-", out]);
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
console.log(`libmpv bundled: ${copied.size} libraries in third_party/mpv/macos-arm64`);
