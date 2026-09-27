// Signing a pending Jellyfin account in, once, during profile selection:
// password or Quick Connect, or skip it for now.
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { Button } from "@/components/tv/Button";
import { Notice } from "@/components/tv/Feedback";
import { ProviderLogo } from "@/components/tv/ServerBadge";
import { TextField } from "@/components/tv/TextField";
import { api, asError } from "@/ipc/api";
import type { ProfileAccount } from "@/ipc/bindings/ProfileAccount";
import { panelSpring } from "@/lib/motion";
import { FocusGroup } from "@/nav/Focusable";

export function AccountSignIn({ account, onDone }: { account: ProfileAccount; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [quick, setQuick] = useState<{ code: string; secret: string } | null>(null);

  const signIn = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.jellyfinLogin(account.baseUrl, account.userName, password);
      onDone();
    } catch (e) {
      setError(asError(e).message);
      setBusy(false);
    }
  };

  const startQuick = async () => {
    setError(null);
    try {
      setQuick(await api.jellyfinQuickConnectStart(account.baseUrl));
    } catch (e) {
      setError(asError(e).message);
    }
  };

  useEffect(() => {
    if (!quick) return;
    const t = setInterval(() => {
      api.jellyfinQuickConnectPoll(account.baseUrl, quick.secret).then(
        (d) => d && onDone(),
        (e) => setError(asError(e).message),
      );
    }, 2000);
    return () => clearInterval(t);
  }, [quick, account.baseUrl, onDone]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 40 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 20 }}
      transition={panelSpring}
      className="glass-strong flex w-[min(30rem,90vw)] flex-col gap-5 rounded-[2rem] p-8"
    >
      <header className="flex items-center gap-2 text-white/70">
        <ProviderLogo kind={account.kind} />
        <span className="font-medium">{account.serverName}</span>
      </header>
      <p className="text-[0.9375rem] text-white/80">
        Sign in as <strong className="text-white">{account.userName}</strong> once; Flick remembers it.
      </p>
      {quick ? (
        <div className="flex flex-col items-center gap-2">
          <p className="text-4xl font-bold tracking-[0.3em] tabular-nums">{quick.code}</p>
          <p className="text-center text-sm text-white/60">Enter this code in Jellyfin, under Quick Connect.</p>
        </div>
      ) : (
        <TextField label="Password" type="password" value={password} onChange={setPassword} autoFocus onEnter={() => void signIn()} />
      )}
      {error && <Notice tone="error">{error}</Notice>}
      <FocusGroup className="flex flex-wrap gap-3">
        {!quick && (
          <Button variant="primary" disabled={busy} onClick={() => void signIn()}>
            Sign In
          </Button>
        )}
        {!quick && <Button onClick={() => void startQuick()}>Quick Connect</Button>}
        <Button variant="ghost" onClick={onDone}>
          Skip
        </Button>
      </FocusGroup>
    </motion.div>
  );
}
