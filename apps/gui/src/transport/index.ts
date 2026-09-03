import type { ClientTransport } from "../protocol";
import { FixtureTransport, type FixtureScenario } from "./fixtureTransport";
import { TauriTransport } from "./tauriTransport";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const fixtureScenarios = new Set<FixtureScenario>([
  "ready",
  "streaming",
  "offline",
  "credential",
  "permission",
  "failed",
  "empty",
]);

export function fixtureScenarioFromLocation(location: Location = window.location): FixtureScenario {
  const requested = new URLSearchParams(location.search).get("fixture") as FixtureScenario | null;
  return requested && fixtureScenarios.has(requested) ? requested : "ready";
}

export function createClientTransport(): ClientTransport {
  if (window.__TAURI_INTERNALS__) {
    return new TauriTransport();
  }
  return new FixtureTransport(fixtureScenarioFromLocation());
}
