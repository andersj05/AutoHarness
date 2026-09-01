export interface MeterProps {
  detail?: string;
  label: string;
  max?: number;
  unavailableLabel?: string;
  value?: number;
}

export function Meter({ detail, label, max = 100, unavailableLabel = "Unavailable", value }: MeterProps) {
  const valid = value !== undefined && Number.isFinite(value) && value >= 0 && max > 0;
  const bounded = valid ? Math.min(value, max) : 0;
  const percent = valid ? Math.round(bounded / max * 100) : undefined;
  return (
    <div className="dsMeter" data-available={valid}>
      <div className="dsMeterHeading"><span>{label}</span><strong>{percent === undefined ? unavailableLabel : `${percent}%`}</strong></div>
      <progress aria-label={label} max={max} value={bounded}>{percent ?? 0}%</progress>
      {detail ? <small>{detail}</small> : null}
    </div>
  );
}
