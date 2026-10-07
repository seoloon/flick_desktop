// Raises Flick's version, checks the project still builds, commits and pushes.
//
//   pnpm bump patch|minor|major
//
// The version lives once, in Cargo.toml [workspace.package]: every crate and
// the app (tauri.conf.json has none) inherit it. Cargo.lock follows from the
// `cargo check`. No tag is made: `pnpm release` creates the GitHub release
// (and its tag) on the pushed commit.
//
// Next steps: `pnpm release` on Windows and on macOS, then
// `pnpm release:publish`.
import { execFileSync, execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const CARGO = join(root, "Cargo.toml");
const LOCK = join(root, "Cargo.lock");

function fail(message) {
  console.error(`\n✗ ${message}`);
  process.exit(1);
}

const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const gitShow = (...args) => execFileSync("git", args, { cwd: root, stdio: "inherit" });
/** Fixed command lines only (no interpolated input): pnpm is a .cmd on Windows, which needs a shell. */
const sh = (command) => execSync(command, { cwd: root, stdio: "inherit" });

/** x.y.z by segment; no pre-release or build metadata. */
export function bump(current, kind) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
  if (!m) throw new Error(`invalid version "${current}" (expected x.y.z)`);
  let [major, minor, patch] = m.slice(1).map(Number);
  if (kind === "major") [major, minor, patch] = [major + 1, 0, 0];
  else if (kind === "minor") [minor, patch] = [minor + 1, 0];
  else if (kind === "patch") patch += 1;
  else throw new Error(`invalid bump "${kind}" (expected patch, minor or major)`);
  return `${major}.${minor}.${patch}`;
}

function main() {
  const kind = process.argv[2];
  if (!["patch", "minor", "major"].includes(kind)) fail("usage: pnpm bump <patch|minor|major>");

  if (git("status", "--porcelain") !== "") fail("the working tree is not clean: commit or stash first.");
  const branch = git("rev-parse", "--abbrev-ref", "HEAD");
  if (branch !== "master" && branch !== "main") fail(`release from master or main (on "${branch}").`);

  const cargo = readFileSync(CARGO, "utf8");
  const section = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]+)(")/.exec(cargo);
  if (!section) fail("no version in Cargo.toml [workspace.package]");
  const current = section[2];
  const next = bump(current, kind);
  console.log(`Flick: ${current} -> ${next}`);
  writeFileSync(CARGO, cargo.replace(section[0], `${section[1]}${next}${section[3]}`));

  try {
    sh("cargo check --workspace");
    sh("pnpm --filter oneshot-ui typecheck");
  } catch {
    console.error("\n✗ a check failed: version change reverted.");
    gitShow("checkout", "--", "Cargo.toml", "Cargo.lock");
    process.exit(1);
  }

  gitShow("add", CARGO, LOCK);
  gitShow("commit", "-m", `Version ${next}`);
  gitShow("push", "origin", branch);

  console.log(`\n✓ Version ${next} committed and pushed on ${branch}.`);
  console.log("Next: `pnpm release` on Windows then on macOS, then `pnpm release:publish`.");
}

main();
