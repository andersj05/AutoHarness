import type { PermissionRequest } from "../protocol";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

interface PermissionDialogProps {
  busy?: boolean;
  permission: PermissionRequest;
  onAllow: () => void;
  onDeny: () => void;
}

export function PermissionDialog({ busy = false, permission, onAllow, onDeny }: PermissionDialogProps) {
  return (
    <Dialog
      description="Review the exact frozen operation. Your answer applies to this call only."
      dismissible={false}
      eyebrow="Permission required"
      footer={
        <>
          <button className="button secondary dangerText" data-initial-focus disabled={busy} onClick={onDeny} type="button">
            Deny operation
          </button>
          <button className="button primary" disabled={busy} onClick={onAllow} type="button">
            <Icon name="shield" size={16} /> {busy ? "Recording answer" : "Allow once"}
          </button>
        </>
      }
      labelledBy="permission-dialog-title"
      title={permission.capability}
    >
      <div className="permissionHero">
        <span className="permissionIcon"><Icon name="shield" size={23} /></span>
        <div>
          <strong>{permission.toolName}</strong>
          <code>{permission.resource}</code>
        </div>
      </div>
      <p className="permissionReason">{permission.reason}</p>
      <dl className="trustedFields">
        {permission.trustedFields.map((field, index) => (
          <div key={`${field.label}-${index}`}>
            <dt>{field.label}</dt>
            <dd>{field.value}</dd>
          </div>
        ))}
      </dl>
      <div className="securityNote">
        <Icon name="shield" size={16} />
        <span>The runtime will persist your answer before this capability can execute.</span>
      </div>
    </Dialog>
  );
}
