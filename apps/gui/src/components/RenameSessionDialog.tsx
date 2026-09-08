import { useId, useState } from "react";
import { MAX_SESSION_TITLE_UTF8_BYTES, type ClientCommand, type CommandOutcome } from "../protocol";
import { Button, Dialog, Field } from "./primitives";

interface Props {
  session: { id: string; title: string };
  onCommand: (command: ClientCommand) => Promise<CommandOutcome>;
  onClose: () => void;
  onRenamed?: (title: string) => void;
}

function titleError(title: string): string | undefined {
  if (!title.trim()) return "Enter a session title.";
  if (/[\u0000-\u001f\u007f]/.test(title)) return "Session titles cannot contain control characters.";
  if (new TextEncoder().encode(title).length > MAX_SESSION_TITLE_UTF8_BYTES) return `Keep the title within ${MAX_SESSION_TITLE_UTF8_BYTES} UTF-8 bytes.`;
  return undefined;
}

export function RenameSessionDialog({ session, onCommand, onClose, onRenamed }: Props) {
  const [title, setTitle] = useState(session.title);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const formId = useId();
  const error = titleError(title);
  const save = async () => {
    if (busy || error || title === session.title) return;
    setBusy(true);
    setMessage("");
    const outcome = await onCommand({ type: "rename_session", sessionId: session.id, title });
    setBusy(false);
    if (outcome === "committed") { onRenamed?.(title); onClose(); }
    else setMessage(outcome === "unknown" ? "The result is uncertain. Close and check the current title before trying again." : "Could not rename this session. Try again.");
  };
  return <Dialog title="Rename session" onClose={() => { if (!busy) onClose(); }} footer={<>
    <Button disabled={busy} onClick={onClose} variant="quiet">Cancel</Button>
    <Button disabled={Boolean(error) || title === session.title} form={formId} loading={busy} loadingLabel="Renaming" type="submit" variant="primary">Save title</Button>
  </>}>
    <form id={formId} onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <Field autoComplete="off" data-initial-focus disabled={busy} error={error} label="New title" onChange={(event) => setTitle(event.target.value)} onFocus={(event) => event.currentTarget.select()} value={title} />
      {message ? <p className="renameSessionStatus" role="status">{message}</p> : null}
    </form>
  </Dialog>;
}
