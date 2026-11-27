import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import App from "./App";
import { Pointer } from "./components/Pointer";

// The same page also draws the on-screen arrows for Ask (a second, see-through window).
const overlay = new URLSearchParams(location.search).get("overlay") === "pointer";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {overlay ? <Pointer /> : <App />}
  </StrictMode>,
);
