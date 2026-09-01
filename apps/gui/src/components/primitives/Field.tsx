import { useId, type InputHTMLAttributes, type ReactNode } from "react";

export interface FieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  error?: string;
  hint?: string;
  label: string;
  leading?: ReactNode;
  trailing?: ReactNode;
}

export function Field({
  className = "",
  error,
  hint,
  id,
  label,
  leading,
  trailing,
  ...inputProps
}: FieldProps) {
  const generatedId = useId();
  const inputId = id ?? generatedId;
  const hintId = hint ? `${inputId}-hint` : undefined;
  const errorId = error ? `${inputId}-error` : undefined;
  const describedBy = [inputProps["aria-describedby"], hintId, errorId].filter(Boolean).join(" ") || undefined;
  return (
    <div className={`dsField ${className}`.trim()} data-invalid={Boolean(error)}>
      <label className="dsFieldLabel" htmlFor={inputId}>{label}</label>
      <span className="dsFieldControl">
        {leading ? <span aria-hidden="true" className="dsFieldAdornment">{leading}</span> : null}
        <input {...inputProps} aria-describedby={describedBy} aria-invalid={Boolean(error) || undefined} id={inputId} />
        {trailing ? <span className="dsFieldAdornment">{trailing}</span> : null}
      </span>
      {hint ? <span className="dsFieldHint" id={hintId}>{hint}</span> : null}
      {error ? <span className="dsFieldError" id={errorId} role="alert">{error}</span> : null}
    </div>
  );
}
