import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const input = document.querySelector("#popup-text");
const results = document.querySelector("#popup-results");
const dictBox = document.querySelector("#popup-dict");
const pin = document.querySelector("#popup-pin");
let pinned = false;
let cards = new Map();
let kind = "translate";

function render(items) {
  results.replaceChildren();
  for (const item of items) {
    const node = document.createElement("article");
    node.className = "result-card";
    const title = document.createElement("strong");
    title.textContent = item.label || item.engine || "译文";
    const body = document.createElement("p");
    body.textContent = item.error || item.text || "";
    const row = document.createElement("div");
    row.className = "row";
    if (item.text) {
      const copy = document.createElement("button");
      copy.className = "subtle";
      copy.type = "button";
      copy.textContent = "复制";
      copy.addEventListener("click", () => navigator.clipboard.writeText(item.text));
      const speak = document.createElement("button");
      speak.className = "subtle";
      speak.type = "button";
      speak.textContent = "朗读";
      speak.addEventListener("click", () => invoke("speak", { text: item.text, lang: "auto" }));
      row.append(copy, speak);
    }
    node.append(title, body, row);
    results.append(node);
  }
}

async function translate(text) {
  cards = new Map();
  render([{ label: "译文", text: "正在翻译…" }]);
  dictBox.hidden = true;
  try {
    const response = await invoke("translate_compare", { text, source: "auto", target: "zh" });
    render(response.results || []);
  } catch (error) {
    render([{ label: "译文", error: String(error) }]);
  }
  if (text.trim().split(/\s+/).length <= 3) {
    invoke("lookup_dict", { text: text.trim() })
      .then((entry) => {
        dictBox.hidden = false;
        dictBox.textContent = `${entry.word} ${entry.phonetic || ""}\n${(entry.meanings || []).slice(0, 3).join("\n")}`;
      })
      .catch(() => {});
  }
}

if ("__TAURI_INTERNALS__" in window) {
listen("engine-result", (event) => {
  cards.set(event.payload.engine, event.payload);
  render([...cards.values()]);
});

listen("popup-open", (event) => {
  const payload = event.payload || {};
  kind = payload.kind || "translate";
  input.value = payload.text || "";
  if (kind === "notice") {
    render([{ label: "从喵翻译", text: payload.text || "" }]);
    return;
  }
  if (kind === "input") {
    input.focus();
    render([]);
    return;
  }
  if (kind === "ocr") {
    render([]);
    const button = document.createElement("button");
    button.className = "accent";
    button.type = "button";
    button.textContent = "翻译";
    button.addEventListener("click", () => translate(input.value));
    results.append(button);
    return;
  }
  if (payload.text) translate(payload.text);
});
}

input.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    translate(input.value);
  }
  if (event.key === "Escape") {
    invoke("app_action", { action: { op: "popup-hide", kind: "", pinned } });
  }
});
let popupAuto = false;
let popupTimer = 0;
if ("__TAURI_INTERNALS__" in window) {
  invoke("get_settings")
    .then((saved) => {
      popupAuto = Boolean(saved.auto_translate);
    })
    .catch(() => {});
}
input.addEventListener("input", () => {
  if (!popupAuto || kind === "ocr" || kind === "notice") return;
  window.clearTimeout(popupTimer);
  popupTimer = window.setTimeout(() => {
    if (input.value.trim()) translate(input.value);
  }, 650);
});
pin.addEventListener("click", () => {
  pinned = !pinned;
  pin.setAttribute("aria-pressed", String(pinned));
  pin.textContent = pinned ? "已固定" : "固定";
});
document.querySelector("#popup-close").addEventListener("click", () => {
  pinned = false;
  invoke("app_action", { action: { op: "popup-hide", kind: "", pinned: false } });
});
window.addEventListener("blur", () => {
  if (!pinned && kind !== "input") {
    invoke("app_action", { action: { op: "popup-hide", kind: "", pinned: false } });
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !pinned) {
    invoke("app_action", { action: { op: "popup-hide", kind: "", pinned: false } });
  }
});
