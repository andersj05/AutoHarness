import type { PermissionRequest } from "../protocol";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
import { Button } from "./primitives";

interface PermissionDialogProps {
  busy?: boolean;
  permission: PermissionRequest;
  onAllow: () => void;
  onDeny: () => void;
}

export function PermissionDialog({ busy = false, permission, onAllow, onDeny }: PermissionDialogProps) {
  return (
    <Dialog
      authority="permission"
      description="Review the exact frozen operation. Your answer applies to this call only."
      dismissible={false}
      eyebrow="Permission required"
      footer={
        <>
          <Button data-initial-focus disabled={busy} onClick={onDeny} variant="danger">Deny operation</Button>
          <Button disabled={busy} icon="shield" onClick={onAllow} variant="primary">{busy ? "Recording answer" : "Allow once"}</Button>
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
