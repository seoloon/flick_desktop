// Glass back arrow for pages reached by drilling in (a title, a library).
// Remote users also have Back (Escape, B); this is for the pointer.
import { ArrowLeft } from "lucide-react";
import { useNavigate } from "react-router";
import { goBack } from "@/lib/history";
import { Button } from "./Button";

export function BackButton({ className }: { className?: string }) {
  const navigate = useNavigate();
  return <Button size="icon" icon={ArrowLeft} label="Back" focusKey="page-back" onClick={() => goBack(navigate)} className={className} />;
}
