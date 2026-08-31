import { useSyncExternalStore } from "react";
import type { ClientStore, ClientStoreSnapshot } from "./clientStore";

export function useClientStore(store: ClientStore): ClientStoreSnapshot {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
