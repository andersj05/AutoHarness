const UNSAFE_SECURITY_DISPLAY =
  /[\\\u0000-\u001f\u007f-\u009f\u00ad\u034f\u061c\u115f-\u1160\u17b4-\u17b5\u180b-\u180f\u200b-\u200f\u202a-\u202e\u2060-\u206f\u3164\ufe00-\ufe0f\ufeff\uffa0\ufff0-\ufffb\u{1bca0}-\u{1bca3}\u{1d173}-\u{1d17a}\u{e0000}-\u{e0fff}]/gu;

export function securityDisplaySafe(value: string): string {
  return value.replace(UNSAFE_SECURITY_DISPLAY, (character) =>
    character === "\\" ? "\\\\" : `\\u{${character.codePointAt(0)!.toString(16)}}`,
  );
}
