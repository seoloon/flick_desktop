// A paragraph cut after a few lines; "More" appears only when text is really
// hidden, and expands it in place.
import { useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { Button } from "./Button";

export function ClampedText({ text, clamp, className }: { text: string; clamp: string; className?: string }) {
  const ref = useRef<HTMLParagraphElement>(null);
  const [more, setMore] = useState(false);
  const [overflows, setOverflows] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setOverflows(el.scrollHeight > el.clientHeight + 1);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [text]);
  return (
    <div className="flex flex-col items-start gap-2">
      <p ref={ref} className={cn(className, more ? "whitespace-pre-line" : clamp)}>
        {text}
      </p>
      {(overflows || more) && (
        <Button size="sm" variant="ghost" onClick={() => setMore(!more)}>
          {more ? "Less" : "More"}
        </Button>
      )}
    </div>
  );
}
