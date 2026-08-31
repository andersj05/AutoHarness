import { useRef, useState, type FormEvent } from "react";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

interface CredentialDialogProps {
  connectionId: string;
  providerLabel: string;
  onClose: () => void;
  onSubmit: (connectionId: string, credential: string) => Promise<boolean>;
}

export function CredentialDialog({ connectionId, providerLabel, onClose, onSubmit }: CredentialDialogProps) {
  const [credential, setCredential] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!credential.trim() || submitting) return;
    const ownedCredential = credential;
    setCredential("");
    if (inputRef.current) inputRef.current.value = "";
    setSubmitting(true);
    const accepted = await onSubmit(connectionId, ownedCredential);
    setSubmitting(false);
    if (accepted) onClose();
  };

  return (
    <Dialog
      description="The value crosses a dedicated one-way boundary and is immediately cleared from the page."
      eyebrow="Ephemeral credential"
      labelledBy="credential-dialog-title"
      onClose={onClose}
      title={`Connect ${providerLabel}`}
    >
      <form className="credentialForm" onSubmit={(event) => void submit(event)}>
        <label>
          <span>Provider credential</span>
          <span className="passwordField">
            <Icon name="shield" size={16} />
            <input
              autoCapitalize="none"
              autoComplete="off"
              autoCorrect="off"
              autoFocus
              data-initial-focus
              onChange={(event) => setCredential(event.target.value)}
              placeholder="Paste credential"
              ref={inputRef}
              spellCheck={false}
              type="password"
              value={credential}
            />
          </span>
        </label>
        <div className="credentialBoundary">
          <Icon name="shield" size={16} />
          <p><strong>Secret-safe ingress</strong><span>No browser storage, transcript, diagnostic, or host snapshot receives this value.</span></p>
        </div>
        <div className="credentialActions">
          <button className="button secondary" disabled={submitting} onClick={onClose} type="button">Cancel</button>
          <button className="button primary" disabled={!credential.trim() || submitting} type="submit">
            {submitting ? "Transferring" : "Connect provider"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
