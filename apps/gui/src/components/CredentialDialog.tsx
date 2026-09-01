import { useRef, useState, type FormEvent } from "react";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
import { Button, Field } from "./primitives";

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
        <Field
          autoCapitalize="none"
          autoComplete="off"
          autoCorrect="off"
          autoFocus
          data-initial-focus
          label="Provider credential"
          leading={<Icon name="shield" size={16} />}
          onChange={(event) => setCredential(event.target.value)}
          placeholder="Paste credential"
          ref={inputRef}
          spellCheck={false}
          type="password"
          value={credential}
        />
        <div className="credentialBoundary">
          <Icon name="shield" size={16} />
          <p><strong>Secret-safe ingress</strong><span>No browser storage, transcript, diagnostic, or host snapshot receives this value.</span></p>
        </div>
        <div className="credentialActions">
          <Button disabled={submitting} onClick={onClose}>Cancel</Button>
          <Button disabled={!credential.trim()} loading={submitting} loadingLabel="Transferring" type="submit" variant="primary">Connect provider</Button>
        </div>
      </form>
    </Dialog>
  );
}
