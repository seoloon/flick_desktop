// What a person reads when something fails: the sentence, then its code
// (`FLK-NET-001`), so a report can be matched with ERROR_IDENTIFIER.md.
import { asError } from "@/ipc/api";

/** Codes of the errors raised by the screens themselves (the rest come from Rust). */
export const UI_CODE = {
  crash: "FLK-UI-001",
  clipboard: "FLK-UI-002",
  linkNotSaved: "FLK-LINK-006",
} as const;

const CODE_AT_END = /\(FLK-[A-Z]+-\d{3}\)\s*$/;

/** `text (CODE)`, unless the text already carries its code. */
export const withCode = (text: string, code: string) => (CODE_AT_END.test(text) ? text : `${text.replace(/\s+$/, "")} (${code})`);

/** An error from a command, as shown to the person: sentence and code. */
export function errorText(e: unknown): string {
  const { message, code } = asError(e);
  return withCode(message, code);
}
