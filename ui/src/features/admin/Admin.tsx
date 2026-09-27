// Server management, limited to what the signed-in account is allowed to do.
// Each section loads independently: a 403 on one is shown in place.
import { useQuery } from "@tanstack/react-query";
import { Play, RefreshCw } from "lucide-react";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/tv/Button";
import { CenteredSpinner, Notice } from "@/components/tv/Feedback";
import { Facts, Page, PageHeader, Panel, Pill } from "@/components/tv/Page";
import { Segmented } from "@/components/tv/Segmented";
import { api, asError, unwrap } from "@/ipc/api";
import type { RustResult } from "@/ipc/app-types";
import { FocusGroup, Screen } from "@/nav/Focusable";
import { librariesQuery, serversQuery } from "@/shell/navItems";

function Section<T>({ title, result, children, className }: { title: string; result: RustResult<T>; children: (v: T) => ReactNode; className?: string }) {
  const r = unwrap(result);
  return (
    <Panel title={title} className={className}>
      {r.ok ? children(r.value) : <p className="text-sm text-muted-foreground">{r.error.message}</p>}
    </Panel>
  );
}

function List({ children }: { children: ReactNode }) {
  return <ul className="flex flex-col divide-y divide-white/[0.07] text-[0.9375rem]">{children}</ul>;
}

export function Admin() {
  const servers = useQuery({ ...serversQuery, select: (list) => list.filter((s) => s.connected && s.server.user.isAdmin) });
  const [picked, setPicked] = useState<string | null>(null);
  const server = picked ?? servers.data?.[0]?.server.id ?? null;
  const overview = useQuery({ queryKey: ["admin", server], queryFn: () => api.adminOverview(server!), enabled: !!server });
  const libs = useQuery(librariesQuery);

  const act = async (label: string, fn: () => Promise<void>) => {
    try {
      await fn();
      toast.success(`${label}: started`);
      setTimeout(() => void overview.refetch(), 1500);
    } catch (e) {
      toast.error(`${label}: ${asError(e).message}`);
    }
  };

  const o = overview.data;
  return (
    <Screen ready={!!servers.data}>
      <Page>
        <PageHeader
          title="Server Management"
          lead="Only actions your account is allowed to perform are shown. The server checks every request."
          actions={
            servers.data && servers.data.length > 1 ? (
              <Segmented options={servers.data.map((s) => ({ value: s.server.id, label: s.server.name }))} value={server ?? ""} onChange={setPicked} label="Server" />
            ) : undefined
          }
        />
        {!servers.data ? (
          <CenteredSpinner />
        ) : servers.data.length === 0 ? (
          <p className="text-lg text-muted-foreground">None of your accounts administers a connected server.</p>
        ) : overview.error ? (
          <Notice tone="error">{asError(overview.error).message}</Notice>
        ) : !o ? (
          <CenteredSpinner />
        ) : (
          <div className="grid grid-cols-[repeat(auto-fit,minmax(24rem,1fr))] gap-5">
            <Section title="Server" result={o.info}>
              {(i) => (
                <Facts
                  rows={[
                    ["Name", i.name],
                    ["Version", <span className="inline-flex items-center gap-2">{i.version} {i.updateAvailable && <Pill tone="warn">Update available</Pill>}</span>],
                    ...(i.os ? ([["System", i.os]] as [string, string][]) : []),
                    ...i.extra,
                  ]}
                />
              )}
            </Section>
            <Section title="Playing Now" result={o.sessions}>
              {(sessions) =>
                sessions.length ? (
                  <List>
                    {sessions.map((s, i) => (
                      <li key={i} className="flex flex-wrap items-center gap-2 py-2.5">
                        <strong>{s.user || "Unknown user"}</strong>
                        <span className="text-muted-foreground">on {s.device || s.client}</span>
                        {s.title && <span className="text-white/85">· {s.title}</span>}
                        {s.transcoding && <Pill tone="warn">Transcoding</Pill>}
                      </li>
                    ))}
                  </List>
                ) : (
                  <p className="text-sm text-muted-foreground">Nobody is watching right now.</p>
                )
              }
            </Section>
            <Panel title="Libraries">
              <FocusGroup className="flex flex-wrap gap-2">
                {(libs.data?.data.find((s) => s.server.id === server)?.libraries ?? []).map((l) => (
                  <Button key={l.id} size="sm" icon={RefreshCw} onClick={() => void act(`Scan ${l.name}`, () => api.adminScanLibrary(l.id))}>
                    Scan {l.name}
                  </Button>
                ))}
              </FocusGroup>
            </Panel>
            <Section title="Scheduled Tasks" result={o.tasks}>
              {(tasks) => (
                <FocusGroup>
                  <List>
                    {tasks.map((t) => (
                      <li key={t.id} className="flex items-center justify-between gap-4 py-2">
                        <span className="flex min-w-0 flex-col">
                          <span className="truncate">{t.name}</span>
                          <span className="truncate text-xs text-muted-foreground">
                            {t.state}
                            {t.progress != null ? ` · ${Math.round(t.progress)} %` : ""}
                            {t.lastResult ? ` · last run: ${t.lastResult}` : ""}
                          </span>
                        </span>
                        <Button variant="ghost" size="sm" icon={Play} onClick={() => void act(t.name, () => api.adminRunTask(server!, t.id))}>
                          Run
                        </Button>
                      </li>
                    ))}
                  </List>
                </FocusGroup>
              )}
            </Section>
            <Section title="Users" result={o.users}>
              {(users) => (
                <List>
                  {users.map((u, i) => (
                    <li key={i} className="flex items-center gap-2 py-2.5">
                      {u.name}
                      {u.isAdmin && <Pill>Administrator</Pill>}
                      {u.isDisabled && <Pill tone="warn">Disabled</Pill>}
                    </li>
                  ))}
                </List>
              )}
            </Section>
            <Section title="Log Files" result={o.logs}>
              {(logs) => (
                <List>
                  {logs.slice(0, 20).map((l) => (
                    <li key={l} className="truncate py-2 font-mono text-xs text-white/80">
                      {l}
                    </li>
                  ))}
                </List>
              )}
            </Section>
          </div>
        )}
      </Page>
    </Screen>
  );
}
