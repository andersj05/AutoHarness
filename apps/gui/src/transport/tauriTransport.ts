import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ClientCommand,
  ClientFrame,
  ClientSnapshot,
  ClientTransport,
  CommandReceipt,
  CredentialSubmission,
} from "../protocol";
import { MAX_PROMPT_UTF8_BYTES } from "../protocol";
import { commandToWire, frameFromWire, receiptFromWire } from "./wireAdapter";
import { WIRE_SCHEMA_VERSION, type WireCommandEnvelope, type WireCommandReceipt, type WireServerFrame } from "./wire";

class TauriCarrierError extends Error {
  constructor(message: string, readonly code?: string) {
    super(message);
    this.name = "TauriCarrierError";
  }
}

function carrierError(error: unknown, fallback: string): TauriCarrierError {
  if (error instanceof TauriCarrierError) return error;
  if (error instanceof Error) return new TauriCarrierError(error.message);
  let message: unknown;
  let code: unknown;
  if (typeof error === "string") message = error;
  else if (typeof error === "object" && error !== null) {
    try {
      message = Reflect.get(error, "message");
      code = Reflect.get(error, "code");
    } catch {
      message = undefined;
      code = undefined;
    }
  }
  const safeCode = typeof code === "string" && /^[a-z][a-z0-9_]{0,63}$/.test(code) ? code : undefined;
  if (
    typeof message === "string" &&
    message.length > 0 &&
    message.length <= 512 &&
    !/[\u0000-\u001f\u007f]/.test(message)
  ) return new TauriCarrierError(message, safeCode);
  return new TauriCarrierError(fallback, safeCode);
}

function isFatalCarrierError(error: TauriCarrierError): boolean {
  return !error.code || !["host_busy", "invalid_command", "invalid_credential"].includes(error.code);
}

function isRustBlank(value: string): boolean {
  for (const character of value) {
    const codePoint = character.codePointAt(0)!;
    if (
      !(
        (codePoint >= 0x0009 && codePoint <= 0x000d) ||
        codePoint === 0x0020 ||
        codePoint === 0x0085 ||
        codePoint === 0x00a0 ||
        codePoint === 0x1680 ||
        (codePoint >= 0x2000 && codePoint <= 0x200a) ||
        codePoint === 0x2028 ||
        codePoint === 0x2029 ||
        codePoint === 0x202f ||
        codePoint === 0x205f ||
        codePoint === 0x3000
      )
    ) return false;
  }
  return true;
}

export class TauriTransport implements ClientTransport {
  private frameChannel?: Channel<WireServerFrame>;
  private onError?: (error: unknown) => void;
  private baselineWaiters: Array<{
    reason: "initial" | "resynchronization";
    resolve: (snapshot: ClientSnapshot) => void;
    reject: (error: unknown) => void;
  }> = [];

  async connect(onFrame: (frame: ClientFrame) => void, onError: (error: unknown) => void): Promise<ClientSnapshot> {
    this.onError = onError;
    const onFrameChannel = new Channel<WireServerFrame>();
    onFrameChannel.onmessage = (frame) => {
      try {
        const mapped = frameFromWire(frame);
        onFrame(mapped);
        void invoke<void>("gui_acknowledge_frame", { revision: frame.revision }).catch((error) => {
          const failure = carrierError(error, "The host did not accept the frame acknowledgement");
          this.rejectWaiters(failure);
          onError(failure);
        });
        if (mapped.kind === "snapshot" && (mapped.reason === "initial" || mapped.reason === "resynchronization")) {
          const index = this.baselineWaiters.findIndex((waiter) => waiter.reason === mapped.reason);
          if (index >= 0) this.baselineWaiters.splice(index, 1)[0]?.resolve(mapped.snapshot);
        }
      } catch (error) {
        this.rejectWaiters(error);
        onError(error);
      }
    };
    this.frameChannel = onFrameChannel;
    const baseline = this.waitForBaseline("initial");
    try {
      const [, snapshot] = await Promise.all([
        invoke<void>("gui_connect", { onFrame: onFrameChannel }),
        baseline,
      ]);
      return snapshot;
    } catch (error) {
      const failure = carrierError(error, "The native host did not accept the renderer connection");
      this.rejectWaiters(failure);
      throw failure;
    }
  }

  command(command: ClientCommand): Promise<CommandReceipt> {
    if (command.type === "submit_prompt") {
      const promptBytes = new TextEncoder().encode(command.prompt).byteLength;
      if (isRustBlank(command.prompt)) {
        return Promise.reject(new TauriCarrierError("Enter a prompt before sending.", "invalid_command"));
      }
      if (promptBytes > MAX_PROMPT_UTF8_BYTES) {
        return Promise.reject(new TauriCarrierError(
          `The prompt is ${String(promptBytes)} UTF-8 bytes. The desktop limit is ${String(MAX_PROMPT_UTF8_BYTES)} bytes.`,
          "invalid_command",
        ));
      }
    }
    return invoke<WireCommandReceipt>("gui_dispatch", { command: commandToWire(command) })
      .then(receiptFromWire)
      .catch((error) => {
        const failure = carrierError(error, "The native host did not accept the command");
        if (isFatalCarrierError(failure)) this.onError?.(failure);
        throw failure;
      });
  }

  async snapshot(lastAppliedRevision?: string): Promise<ClientSnapshot> {
    const baseline = this.waitForBaseline("resynchronization");
    try {
      const command: WireCommandEnvelope = {
        schema_version: WIRE_SCHEMA_VERSION,
        command: {
          kind: "request_resynchronization",
          payload: { last_applied_revision: lastAppliedRevision ?? null },
        },
      };
      const [, snapshot] = await Promise.all([
        invoke<WireCommandReceipt>("gui_dispatch", { command }),
        baseline,
      ]);
      return snapshot;
    } catch (error) {
      const failure = carrierError(error, "The native host did not publish a recovery snapshot");
      this.rejectWaiters(failure);
      throw failure;
    }
  }

  submitCredential(secret: CredentialSubmission): Promise<CommandReceipt> {
    return invoke<WireCommandReceipt>("gui_submit_credential", {
      connectionId: secret.connectionId,
      operation: secret.operation,
      credential: secret.credential,
    }).then(receiptFromWire).catch((error) => {
      const failure = carrierError(error, "The native host did not accept the credential");
      if (isFatalCarrierError(failure)) this.onError?.(failure);
      throw failure;
    });
  }

  async close(): Promise<void> {
    if (this.frameChannel) {
      const command: WireCommandEnvelope = {
        schema_version: WIRE_SCHEMA_VERSION,
        command: { kind: "request_shutdown" },
      };
      await invoke<WireCommandReceipt>("gui_dispatch", { command }).catch(() => undefined);
    }
    this.frameChannel = undefined;
    this.onError = undefined;
    this.rejectWaiters(new Error("Tauri transport closed"));
  }

  private waitForBaseline(reason: "initial" | "resynchronization"): Promise<ClientSnapshot> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const index = this.baselineWaiters.findIndex((waiter) => waiter.resolve === resolveWithTimeout);
        if (index >= 0) this.baselineWaiters.splice(index, 1);
        reject(new Error(`The host did not publish a ${reason} frame within 10 seconds`));
      }, 10_000);
      const resolveWithTimeout = (snapshot: ClientSnapshot) => {
        clearTimeout(timer);
        resolve(snapshot);
      };
      const rejectWithTimeout = (error: unknown) => {
        clearTimeout(timer);
        reject(error);
      };
      this.baselineWaiters.push({ reason, resolve: resolveWithTimeout, reject: rejectWithTimeout });
    });
  }

  private rejectWaiters(error: unknown): void {
    const waiters = this.baselineWaiters.splice(0);
    waiters.forEach((waiter) => waiter.reject(error));
  }
}
