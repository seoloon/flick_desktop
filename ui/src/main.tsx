import "./index.css";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { setFrame } from "./lib/mode";
import { getSettings, loadSettings } from "./lib/settings";
import { installGamepad, installKeyboard } from "./nav/input";
import { installSpatialNavigation } from "./nav/spatial";
import { isMac } from "./shell/TitleBar";

async function boot() {
  if (isMac) document.documentElement.dataset.platform = "mac";
  installSpatialNavigation();
  installKeyboard();
  const s = await loadSettings().catch((e) => {
    console.error("settings load failed", e);
    return null;
  });
  installGamepad(
    () => getSettings()?.controller.deadzone ?? 0.35,
    () => getSettings()?.controller.swapConfirm ?? false,
    () => getSettings()?.controller.enabled ?? true,
  );
  if (s?.general.startInMaxiFrame) void setFrame(true);

  // No StrictMode: its dev double-mount would start, stop and restart the
  // native player on every playback.
  createRoot(document.getElementById("root")!).render(<App />);
}

void boot();
