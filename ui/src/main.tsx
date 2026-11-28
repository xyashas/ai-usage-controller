import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import App from "./App";
import { Buddy } from "./components/Buddy";

// The same page also draws the on-screen buddy (a second, see-through window).
const overlay = new URLSearchParams(location.search).get("overlay") === "buddy";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {overlay ? <Buddy /> : <App />}
  </StrictMode>,
);
