import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ClientStore } from "./store/clientStore";
import { createClientTransport } from "./transport";
import "./styles.css";

const store = new ClientStore(createClientTransport());
const root = document.getElementById("root");

if (!root) throw new Error("Missing AutoHarness application root");

createRoot(root).render(
  <StrictMode>
    <App store={store} />
  </StrictMode>,
);
