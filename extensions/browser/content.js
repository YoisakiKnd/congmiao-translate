const root = document.createElement("div");
const shadow = root.attachShadow({ mode: "open" });
shadow.innerHTML = `
  <style>
    :host { all: initial; }
    .wrap {
      position: fixed;
      z-index: 2147483647;
      font-family: "Segoe UI Variable", "Segoe UI", "PingFang SC", "Microsoft YaHei UI", sans-serif;
    }
    button, .panel {
      color: #1a1a1a;
      background: #fff;
      border: 1px solid rgba(0, 0, 0, 0.08);
      box-shadow: 0 8px 16px rgba(0, 0, 0, 0.14);
    }
    button {
      height: 28px;
      padding: 0 12px;
      border-radius: 4px;
      border-color: transparent;
      background: #005fb8;
      color: #fff;
      cursor: pointer;
      font-size: 12px;
    }
    .panel {
      max-width: 320px;
      border-radius: 8px;
      padding: 10px 12px;
      font-size: 14px;
      line-height: 20px;
      white-space: pre-wrap;
    }
  </style>
  <div class="wrap" hidden>
    <button type="button" id="selection">翻译</button>
    <button type="button" id="page" style="margin-left:8px">整页</button>
    <div class="panel" hidden></div>
  </div>
`;
document.documentElement.append(root);

const wrap = shadow.querySelector(".wrap");
const button = shadow.querySelector("#selection");
const pageButton = shadow.querySelector("#page");
const panel = shadow.querySelector(".panel");
let selected = "";

function hide() {
  wrap.hidden = true;
  panel.hidden = true;
  selected = "";
}

function place(rect) {
  wrap.hidden = false;
  wrap.style.left = `${Math.max(8, rect.left)}px`;
  wrap.style.top = `${Math.max(8, rect.bottom + 8)}px`;
}

document.addEventListener("mouseup", (event) => {
  if (event.composedPath().includes(root)) return;
  window.setTimeout(() => {
    const selection = window.getSelection();
    const text = selection?.toString().trim() ?? "";
    if (!text || !selection.rangeCount) {
      if (!panel.hidden) return;
      hide();
      return;
    }
    selected = text;
    panel.hidden = true;
    button.hidden = false;
    place(selection.getRangeAt(0).getBoundingClientRect());
  }, 0);
});

button.addEventListener("mousedown", (event) => event.preventDefault());
button.addEventListener("click", async () => {
  const text = selected;
  if (!text) return;
  button.hidden = true;
  panel.hidden = false;
  panel.textContent = "正在翻译…";
  const stored = await chrome.storage.local.get({ targetLang: "zh" });
  const response = await chrome.runtime.sendMessage({
    type: "translate",
    text,
    source: "auto",
    target: stored.targetLang,
  });
  panel.textContent = response?.ok
    ? (response.results || []).map((item) => `${item.label || item.engine}: ${item.error || item.text}`).join("\n") || response.text
    : `${response?.message || "翻译失败"}\n若桌面端没启动，打开 congmiao://translate`;
});

chrome.runtime.onMessage.addListener((message) => {
  if (message?.type === "show-translation" && message.text) {
    selected = message.text;
    button.hidden = true;
    panel.hidden = false;
    wrap.hidden = false;
    panel.textContent = "正在翻译…";
    chrome.storage.local.get({ targetLang: "zh" }, (stored) => {
      chrome.runtime.sendMessage(
        { type: "compare", text: message.text, source: "auto", target: stored.targetLang },
        (response) => {
          panel.textContent = response?.ok ? response.text : response?.message || "翻译失败";
        },
      );
    });
  }
  if (message?.type === "translate-page") translatePage();
});

async function translatePage() {
  const stored = await chrome.storage.local.get({ targetLang: "zh", pageTranslated: false });
  if (stored.pageTranslated || document.querySelector("[data-congmiao-translation]")) {
    document.querySelectorAll("[data-congmiao-translation]").forEach((node) => node.remove());
    document.querySelectorAll("[data-congmiao]").forEach((node) => delete node.dataset.congmiao);
    await chrome.storage.local.set({ pageTranslated: false });
    pageButton.textContent = "整页";
    return;
  }
  const nodes = [...document.querySelectorAll("p, li, h1, h2, h3, blockquote")]
    .filter((node) => {
      if (node.dataset.congmiao || !node.innerText.trim()) return false;
      const rect = node.getBoundingClientRect();
      return rect.width > 0 && rect.bottom > 0 && rect.top < window.innerHeight;
    })
    .slice(0, 80);
  for (let index = 0; index < nodes.length; index += 8) {
    const chunk = nodes.slice(index, index + 8);
    const response = await chrome.runtime.sendMessage({
      type: "batch",
      texts: chunk.map((node) => node.innerText.trim()),
      source: "auto",
      target: stored.targetLang,
    });
    if (!response?.ok) continue;
    chunk.forEach((node, offset) => {
      const text = response.texts?.[offset];
      if (!text) return;
      const translated = document.createElement("div");
      translated.dataset.congmiaoTranslation = "1";
      translated.style.opacity = "0.8";
      translated.textContent = text;
      node.dataset.congmiao = "1";
      node.insertAdjacentElement("afterend", translated);
    });
  }
  await chrome.storage.local.set({ pageTranslated: true });
  pageButton.textContent = "关闭整页";
}

pageButton.addEventListener("mousedown", (event) => event.preventDefault());
pageButton.addEventListener("click", () => {
  translatePage();
});

document.addEventListener("keydown", (event) => {
  if (event.altKey && event.shiftKey && event.key.toLowerCase() === "t") {
    event.preventDefault();
    translatePage();
  }
});

document.addEventListener("mousedown", (event) => {
  if (event.composedPath().includes(root)) return;
  if (panel.hidden) hide();
});
