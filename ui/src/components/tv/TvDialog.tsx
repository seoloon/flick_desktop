// Modal on frosted glass. Radix (via animate-ui) handles portal, aria and
// focus trapping; Norigin navigation is fenced in by a focus boundary; Back
// (Escape, B, remote back) closes it and focus returns where it was.
import { getCurrentFocusKey } from "@noriginmedia/norigin-spatial-navigation";
import { X } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
} from "@/components/animate-ui/primitives/radix/dialog";
import { panelSpring } from "@/lib/motion";
import { FocusGroup } from "@/nav/Focusable";
import { onAction } from "@/nav/input";
import { focusKey } from "@/nav/spatial";
import { Button } from "./Button";

type Props = {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: ReactNode;
  /** Above the title. */
  icon?: ReactNode;
  /** `false`: Back, the overlay and the close button do nothing (work in progress). */
  dismissible?: boolean;
  children: ReactNode;
};

export function TvDialog({ open, onClose, title, description, icon, dismissible = true, children }: Props) {
  const restore = useRef<string | null>(null);
  const close = useRef(onClose);
  close.current = dismissible ? onClose : () => {};

  useEffect(() => {
    if (!open) return;
    restore.current = getCurrentFocusKey();
    return onAction((a) => {
      if (a.type !== "back") return false;
      close.current();
      return true;
    });
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close.current()}>
      <DialogPortal>
        <DialogOverlay className="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm" />
        <DialogContent
          onOpenAutoFocus={(e) => e.preventDefault()}
          onCloseAutoFocus={(e) => {
            e.preventDefault();
            if (restore.current) focusKey(restore.current);
          }}
          onEscapeKeyDown={(e) => e.preventDefault()}
          initial={{ opacity: 0, scale: 0.94, filter: "blur(8px)" }}
          animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
          exit={{ opacity: 0, scale: 0.96, filter: "blur(6px)" }}
          transition={panelSpring}
          className="glass-strong fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100vh-4rem)] w-[min(38rem,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 flex-col overflow-y-auto rounded-[2rem] p-8 text-white outline-none"
        >
          <FocusGroup focusKey="dialog" boundary autoFocus className="flex flex-col gap-5">
            <header className="flex items-start justify-between gap-4">
              <div className="flex flex-col gap-1.5">
                {icon}
                <DialogTitle className="text-2xl font-bold tracking-tight">{title}</DialogTitle>
                {description ? (
                  <DialogDescription className="text-[0.9375rem] leading-relaxed text-muted-foreground">{description}</DialogDescription>
                ) : (
                  <DialogDescription className="sr-only">{title}</DialogDescription>
                )}
              </div>
              {/* Centred on the corner's arc (32 px radius, 22 px button): concentric with the card. */}
              {dismissible && <Button variant="ghost" size="icon" icon={X} label="Close" onClick={onClose} className="-mt-[1.375rem] -mr-[1.375rem]" />}
            </header>
            {children}
          </FocusGroup>
        </DialogContent>
      </DialogPortal>
    </Dialog>
  );
}
