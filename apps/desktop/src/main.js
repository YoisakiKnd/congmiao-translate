import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const LANGUAGES = [
  ["auto", "自动检测"],
  ["zh", "中文"],
  ["en", "英语"],
  ["ja", "日语"],
  ["ko", "韩语"],
  ["fr", "法语"],
  ["de", "德语"],
  ["es", "西班牙语"],
  ["ru", "俄语"],
];

const I18N = {
  en: {
    app: "Congmiao",
    "nav-translate": "Translate",
    "nav-files": "Files",
    "nav-minecraft": "Mod translation",
    "nav-history": "History",
    "nav-vocab": "Vocabulary",
    "nav-settings": "Settings",
    source: "Source",
    target: "Translation",
    swap: "Swap",
    speak: "Speak",
    translate: "Translate",
    search: "Search",
    clear: "Clear",
    cancel: "Cancel",
    resume: "Resume",
    "open-folder": "Open output folder",
    "file-start": "Translate file",
    "file-path-label": "File path",
    engine: "Engine",
    "output-mode": "Output",
    "mode-translated": "Translation only",
    "mode-bilingual": "Original then translation",
    "instance-path": "Instance folder",
    "game-version": "Game version",
    preview: "Preview",
    "mc-start": "Start",
    "mc-glossary": "Extra glossary, one source=translation per line",
    "history-any-target": "Any target language",
    "history-search-placeholder": "Search history",
    "swap-when-same": "If the source is already the target language, translate into the other language",
    "watch-clipboard": "Translate copied text",
    "auto-translate": "Translate after you pause typing",
    "auto-copy": "Copy the first translation",
    "launch-at-login": "Launch at login",
    "show-tray": "Show tray icon",
    "ui-language": "Interface language",
    theme: "Theme",
    "theme-system": "System",
    "theme-light": "Light",
    "theme-dark": "Dark",
    proxy: "Proxy",
    "job-concurrency": "File translation concurrency",
    "prompt-title": "Translation style",
    prompt: "File jobs, mod translation, and model engines all use this instruction",
    "glossary-title": "Glossary",
    "theme-dark-action": "Dark mode",
    "theme-light-action": "Light mode",
  },
};

const sourceSelect = document.querySelector("#source");
const targetSelect = document.querySelector("#target");
const sourceText = document.querySelector("#source-text");
const results = document.querySelector("#results");
const dictBox = document.querySelector("#dict");
const status = document.querySelector("#status");
const themeToggle = document.querySelector("#theme-toggle");
const THEME_KEY = "congmiao-theme";
let settings = null;
let pinnedCards = new Map();

for (const [code, name] of LANGUAGES) {
  sourceSelect.append(new Option(name, code));
  if (code !== "auto") {
    targetSelect.append(new Option(name, code));
    document.querySelector("#file-target").append(new Option(name, code));
    document.querySelector("#history-target").append(new Option(name, code));
  }
}
sourceSelect.value = "auto";
targetSelect.value = "zh";

function inDesktop() {
  return "__TAURI_INTERNALS__" in window;
}

function applyI18n(lang) {
  const table = I18N[lang] || {};
  document.documentElement.lang = lang === "en" ? "en" : "zh-CN";
  for (const node of document.querySelectorAll("[data-i18n]")) {
    const key = node.dataset.i18n;
    if (!table[key]) continue;
    const text = [...node.childNodes].find((item) => item.nodeType === Node.TEXT_NODE && item.textContent.trim());
    if (text && node.childElementCount > 0) text.textContent = `${table[key]} `;
    else if (node.childElementCount === 0) node.textContent = table[key];
  }
  for (const node of document.querySelectorAll("[data-i18n-placeholder]")) {
    const key = node.dataset.i18nPlaceholder;
    if (table[key]) node.placeholder = table[key];
  }
  applyTheme(localStorage.getItem(THEME_KEY) || "system");
}

function applyTheme(theme) {
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const dark = theme === "dark" || (theme === "system" && systemDark);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  const english = document.documentElement.lang === "en";
  themeToggle.textContent = dark
    ? english
      ? "Light mode"
      : "白天模式"
    : english
      ? "Dark mode"
      : "黑夜模式";
  themeToggle.setAttribute("aria-pressed", String(dark));
  localStorage.setItem(THEME_KEY, theme);
}

function showPage(page) {
  for (const button of document.querySelectorAll(".nav-item[data-page]")) {
    if (button.dataset.page === page) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  }
  for (const section of document.querySelectorAll(".content .page")) {
    section.hidden = section.id !== `page-${page}`;
  }
  if (page === "history") loadHistory().catch(showStatus);
  if (page === "vocab") loadVocab().catch(showStatus);
  if (page === "settings") loadPermissions().catch(showStatus);
}

function showStatus(error) {
  status.textContent = String(error);
}

function card(title, body, extra = "") {
  const node = document.createElement("article");
  node.className = "result-card";
  node.innerHTML = `<header><strong></strong><span class="meta"></span></header><p></p><div class="row"></div>`;
  node.querySelector("strong").textContent = title;
  node.querySelector(".meta").textContent = extra;
  node.querySelector("p").textContent = body;
  return node;
}

function addActions(node, text) {
  const row = node.querySelector(".row");
  const copy = document.createElement("button");
  copy.className = "subtle";
  copy.type = "button";
  copy.textContent = "复制";
  copy.addEventListener("click", () => navigator.clipboard.writeText(text));
  const speak = document.createElement("button");
  speak.className = "subtle";
  speak.type = "button";
  speak.textContent = "朗读";
  speak.addEventListener("click", () => speakText(text, targetSelect.value));
  row.append(copy, speak);
  return node;
}

async function speakText(text, lang) {
  if (!inDesktop()) return;
  const spoken = await invoke("speak", { text, lang });
  if (spoken?.audio_url) {
    const audio = new Audio(spoken.audio_url);
    audio.play().catch(() => {});
  }
}

function renderResults(items) {
  results.replaceChildren();
  if (!items.length) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "译文会显示在这里";
    results.append(empty);
    return;
  }
  for (const item of items) {
    const title = item.label || item.engine;
    const extra = item.unofficial ? "非官方接口，可能失效" : item.cache_hit ? "缓存" : "";
    const body = item.error || item.text || "没有返回译文";
    const node = addActions(card(title, body, extra), item.text || "");
    if (!item.error && item.text) {
      const save = document.createElement("button");
      save.className = "subtle";
      save.type = "button";
      save.textContent = "加入生词本";
      save.addEventListener("click", () =>
        saveWord(sourceText.value.trim(), item.text).catch(showStatus),
      );
      node.querySelector(".row").append(save);
    }
    results.append(node);
  }
}

async function translate() {
  const text = sourceText.value;
  if (!text.trim()) {
    renderResults([{ label: "译文", text: "先输入要翻译的文字", error: "先输入要翻译的文字" }]);
    return;
  }
  if (!inDesktop()) {
    renderResults([{ label: "译文", error: "预览模式不能调用本机翻译服务" }]);
    return;
  }
  pinnedCards = new Map();
  renderResults([{ label: "译文", text: "正在翻译…" }]);
  lookup(text).catch(() => {});
  try {
    const response = await invoke("translate_compare", {
      text,
      source: sourceSelect.value,
      target: targetSelect.value,
    });
    renderResults(response.results || []);
  } catch (error) {
    renderResults([{ label: "译文", error: String(error) }]);
  }
}

async function lookup(text) {
  dictBox.hidden = true;
  if (!inDesktop() || text.trim().split(/\s+/).length > 3) return;
  const entry = await invoke("lookup_dict", { text: text.trim() });
  dictBox.hidden = false;
  dictBox.replaceChildren();
  const title = document.createElement("strong");
  title.textContent = `${entry.word} ${entry.phonetic || ""}`.trim();
  const meanings = document.createElement("p");
  meanings.textContent = (entry.meanings || []).slice(0, 4).join("\n");
  const save = document.createElement("button");
  save.className = "subtle";
  save.type = "button";
  save.textContent = "加入生词本";
  save.addEventListener("click", () =>
    saveWord(entry.word, (entry.meanings || [])[0] || "", entry.phonetic || "").catch(showStatus),
  );
  dictBox.append(title, meanings, save);
}

async function saveWord(word, translation, phonetic = "") {
  if (!word || !inDesktop()) return;
  await invoke("vocabulary_action", {
    action: { op: "add", word, translation, phonetic, format: "" },
  });
  status.textContent = "已加入生词本";
}

async function loadHistory() {
  if (!inDesktop()) return;
  const query = document.querySelector("#history-query").value;
  const items = await invoke("history_action", { action: { op: "list", query, id: 0 } });
  const list = document.querySelector("#history-list");
  const target = document.querySelector("#history-target").value;
  list.replaceChildren();
  for (const item of items) {
    if (target && item.target !== target) continue;
    const node = document.createElement("article");
    node.className = "result-card";
    const translated = (item.results || []).map((result) => result.translated).filter(Boolean).join(" / ");
    node.innerHTML = `<header><strong></strong></header><p></p>`;
    node.querySelector("strong").textContent = item.text;
    node.querySelector("p").textContent = translated;
    const again = document.createElement("button");
    again.className = "subtle";
    again.type = "button";
    again.textContent = "再次翻译";
    again.addEventListener("click", () => {
      sourceText.value = item.text;
      showPage("translate");
      translate();
    });
    const remove = document.createElement("button");
    remove.className = "subtle";
    remove.type = "button";
    remove.textContent = "删除";
    remove.addEventListener("click", async () => {
      await invoke("history_action", { action: { op: "delete", id: item.id, query: "" } });
      loadHistory().catch(showStatus);
    });
    const row = document.createElement("div");
    row.className = "row";
    row.append(again, remove);
    node.append(row);
    list.append(node);
  }
  if (!items.length) list.append(card("历史", "还没有翻译记录"));
}

async function loadVocab() {
  if (!inDesktop()) return;
  const items = await invoke("vocabulary_action", {
    action: { op: "list", id: 0, word: "", translation: "", phonetic: "", format: "" },
  });
  const list = document.querySelector("#vocab-list");
  list.replaceChildren();
  for (const item of items) {
    const node = card(item.word, item.translation, item.phonetic || "");
    const remove = document.createElement("button");
    remove.className = "subtle";
    remove.type = "button";
    remove.textContent = "删除";
    remove.addEventListener("click", async () => {
      await invoke("vocabulary_action", {
        action: { op: "delete", id: item.id, word: "", translation: "", phonetic: "", format: "" },
      });
      loadVocab().catch(showStatus);
    });
    node.querySelector(".row").append(remove);
    list.append(node);
  }
  if (!items.length) list.append(card("生词本", "还没有生词"));
}

async function exportVocab(format) {
  const exported = await invoke("vocabulary_action", {
    action: { op: "export", format, id: 0, word: "", translation: "", phonetic: "" },
  });
  const blob = new Blob([exported.text], { type: "text/plain;charset=utf-8" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = format === "anki" ? "congmiao-vocab.txt" : "congmiao-vocab.csv";
  link.click();
}

function renderEngines() {
  const list = document.querySelector("#engine-list");
  list.replaceChildren();
  for (const engine of settings?.engines || []) {
    const node = document.createElement("article");
    node.className = "result-card";
    const title = document.createElement("label");
    title.className = "check";
    const enabled = document.createElement("input");
    enabled.type = "checkbox";
    enabled.checked = engine.enabled;
    enabled.addEventListener("change", () => {
      engine.enabled = enabled.checked;
    });
    title.append(enabled, document.createTextNode(` ${engine.label || engine.kind}`));
    if (engine.unofficial) {
      const note = document.createElement("span");
      note.className = "meta";
      note.textContent = " 非官方接口，可能失效";
      title.append(note);
    }
    node.append(title);
    const fields = [
      ["base_url", "接口地址"],
      ["api_key", "API Key"],
      ["model", "模型"],
      ["app_id", "App ID"],
      ["secret", "Secret"],
      ["region", "区域"],
    ];
    for (const [key, label] of fields) {
      if (!engine.needs_key && key !== "base_url" && key !== "model") continue;
      if (!engine.needs_key && engine.kind !== "openai" && engine.kind !== "gemini" && engine.kind !== "deepl") {
        continue;
      }
      const field = document.createElement("label");
      field.textContent = label;
      const input = document.createElement("input");
      input.type = key === "api_key" || key === "secret" ? "password" : "text";
      input.value = engine[key] || "";
      input.addEventListener("input", () => {
        engine[key] = input.value;
      });
      field.append(input);
      node.append(field);
    }
    const test = document.createElement("button");
    test.className = "subtle";
    test.type = "button";
    test.textContent = "测试连接";
    const message = document.createElement("p");
    message.className = "meta";
    test.addEventListener("click", async () => {
      message.textContent = "正在测试…";
      try {
        await saveSettings();
        const output = await invoke("test_engine", { kind: engine.kind });
        message.textContent = output.error || output.text;
      } catch (error) {
        message.textContent = String(error);
      }
    });
    node.append(test, message);
    const order = document.createElement("div");
    order.className = "row";
    const up = document.createElement("button");
    up.className = "subtle";
    up.type = "button";
    up.textContent = "上移";
    up.addEventListener("click", () => moveEngine(engine, -1));
    const down = document.createElement("button");
    down.className = "subtle";
    down.type = "button";
    down.textContent = "下移";
    down.addEventListener("click", () => moveEngine(engine, 1));
    order.append(up, down);
    node.append(order);
    list.append(node);
  }
}

function moveEngine(engine, direction) {
  const list = settings?.engines || [];
  const index = list.indexOf(engine);
  const next = index + direction;
  if (index < 0 || next < 0 || next >= list.length) return;
  const [item] = list.splice(index, 1);
  list.splice(next, 0, item);
  renderEngines();
}

function fillSettings(saved) {
  settings = saved;
  for (const engine of settings.engines || []) {
    engine.unofficial = ["google", "bing", "deepl_free", "youdao_web"].includes(engine.kind);
    engine.needs_key = !["echo", "google", "bing", "deepl_free", "youdao_web"].includes(engine.kind);
    const labels = {
      echo: "回声",
      openai: "OpenAI 兼容",
      google: "Google",
      bing: "Bing",
      deepl_free: "DeepL",
      youdao_web: "有道",
      deepl: "DeepL API",
      baidu: "百度翻译",
      tencent: "腾讯翻译",
      alibaba: "阿里翻译",
      youdao: "有道智云",
      azure: "Azure 翻译",
      gemini: "Gemini",
    };
    engine.label = labels[engine.kind] || engine.kind;
  }
  document.querySelector("#swap-when-same").checked = saved.swap_when_same;
  document.querySelector("#watch-clipboard").checked = saved.watch_clipboard;
  document.querySelector("#auto-copy").checked = saved.auto_copy;
  document.querySelector("#launch-at-login").checked = saved.launch_at_login;
  document.querySelector("#show-tray").checked = saved.show_tray;
  document.querySelector("#ui-language").value = saved.ui_language || "zh";
  document.querySelector("#glossary").value = saved.glossary_text || "";
  document.querySelector("#prompt").value = saved.prompt || "";
  document.querySelector("#proxy").value = saved.proxy || "";
  document.querySelector("#job-concurrency").value = String(saved.job_concurrency || 3);
  document.querySelector("#auto-translate").checked = Boolean(saved.auto_translate);
  document.querySelector("#file-target").value = saved.default_target || "zh";
  document.querySelector("#data-dir").textContent = saved.data_dir || "";
  document.querySelector("#shortcut-screenshot").value = saved.shortcuts.screenshot;
  document.querySelector("#shortcut-selection").value = saved.shortcuts.selection;
  document.querySelector("#shortcut-input").value = saved.shortcuts.input;
  document.querySelector("#shortcut-replace").value = saved.shortcuts.replace;
  document.querySelector("#shortcut-silent").value = saved.shortcuts.silent_ocr;
  document.querySelector("#shortcut-hint").textContent =
    `截图 ${saved.shortcuts.screenshot} · 划词 ${saved.shortcuts.selection} · 输入 ${saved.shortcuts.input}`;
  const wayland = document.querySelector("#wayland-shortcut-hint");
  wayland.hidden = !saved.wayland_hint;
  wayland.textContent = saved.wayland_hint || "";
  if (saved.default_source) sourceSelect.value = saved.default_source;
  if (saved.default_target) targetSelect.value = saved.default_target;
  applyI18n(saved.ui_language);
  renderEngines();
  fillEngineSelect("#file-engine");
  fillEngineSelect("#mc-engine");
  if (!saved.onboarded) openOnboarding();
}

function fillEngineSelect(selector) {
  const select = document.querySelector(selector);
  const current = select.value;
  select.replaceChildren();
  for (const engine of settings?.engines || []) {
    select.append(new Option(engine.label || engine.kind, engine.kind));
  }
  if ([...select.options].some((option) => option.value === current)) select.value = current;
}

function collectSettings() {
  return {
    engines: (settings?.engines || []).map((engine) => ({
      kind: engine.kind,
      enabled: engine.enabled,
      base_url: engine.base_url || "",
      api_key: engine.api_key || "",
      model: engine.model || "",
      app_id: engine.app_id || "",
      secret: engine.secret || "",
      region: engine.region || "",
    })),
    default_source: sourceSelect.value,
    default_target: targetSelect.value,
    glossary_text: document.querySelector("#glossary").value,
    swap_when_same: document.querySelector("#swap-when-same").checked,
    shortcuts: {
      screenshot: document.querySelector("#shortcut-screenshot").value,
      selection: document.querySelector("#shortcut-selection").value,
      input: document.querySelector("#shortcut-input").value,
      replace: document.querySelector("#shortcut-replace").value,
      silent_ocr: document.querySelector("#shortcut-silent").value,
    },
    watch_clipboard: document.querySelector("#watch-clipboard").checked,
    auto_translate: document.querySelector("#auto-translate").checked,
    auto_copy: document.querySelector("#auto-copy").checked,
    launch_at_login: document.querySelector("#launch-at-login").checked,
    ui_language: document.querySelector("#ui-language").value,
    show_tray: document.querySelector("#show-tray").checked,
    onboarded: true,
    prompt: document.querySelector("#prompt").value,
    proxy: document.querySelector("#proxy").value,
    job_concurrency: Number(document.querySelector("#job-concurrency").value) || 3,
  };
}

async function saveSettings() {
  const input = collectSettings();
  const values = Object.values(input.shortcuts);
  if (new Set(values).size !== values.length) throw new Error("快捷键冲突");
  await invoke("save_settings", { input });
  settings = { ...settings, ...input, glossary_text: input.glossary_text };
  status.textContent = "设置已保存";
}

async function loadPermissions() {
  if (!inDesktop()) return;
  const info = await invoke("app_action", { action: { op: "info", kind: "", pinned: false } });
  const list = document.querySelector("#permission-list");
  list.replaceChildren();
  const rows = [
    ["辅助功能", info.accessibility, "request-accessibility"],
    ["屏幕录制", info.screen_recording, "request-screen"],
  ];
  for (const [name, granted, op] of rows) {
    const node = card(name, granted ? "已授权" : "未授权");
    const button = document.createElement("button");
    button.className = "subtle";
    button.type = "button";
    button.textContent = "去授权";
    button.addEventListener("click", () =>
      invoke("app_action", { action: { op, kind: "", pinned: false } }).then(loadPermissions),
    );
    node.querySelector(".row").append(button);
    list.append(node);
  }
  document.querySelector("#about-version").textContent = `版本 ${info.version}`;
}

let onboardStep = 0;
function openOnboarding() {
  onboardStep = 0;
  document.querySelector("#onboarding").hidden = false;
  renderOnboarding();
}

function renderOnboarding() {
  const copy = document.querySelector("#onboard-copy");
  const body = document.querySelector("#onboard-body");
  body.replaceChildren();
  const steps = [
    "选择默认目标语言。",
    "确认免 Key 引擎。它们是非官方接口，可能失效。",
    "授予辅助功能和屏幕录制，划词和截图才能工作。",
  ];
  copy.textContent = steps[onboardStep];
  document.querySelector("#onboard-back").hidden = onboardStep === 0;
  document.querySelector("#onboard-next").textContent = onboardStep === 2 ? "完成" : "下一步";
  if (onboardStep === 0) {
    const select = document.createElement("select");
    select.id = "onboard-target";
    for (const option of targetSelect.options) {
      select.append(new Option(option.textContent, option.value));
    }
    select.value = targetSelect.value;
    body.append(select);
  }
  if (onboardStep === 2) {
    const button = document.createElement("button");
    button.className = "subtle";
    button.type = "button";
    button.textContent = "去授权";
    button.addEventListener("click", () =>
      invoke("app_action", { action: { op: "request-accessibility", kind: "", pinned: false } }),
    );
    body.append(button);
  }
}

document.querySelectorAll(".nav-item[data-page]").forEach((button) => {
  button.addEventListener("click", () => showPage(button.dataset.page));
});
document.querySelectorAll(".tabs button").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll(".tabs button").forEach((item) => {
      item.setAttribute("aria-selected", String(item === button));
    });
    document.querySelectorAll("[data-panel]").forEach((panel) => {
      panel.hidden = panel.dataset.panel !== button.dataset.tab;
    });
  });
});
document.querySelector("#translate").addEventListener("click", translate);
document.querySelector("#swap").addEventListener("click", () => {
  if (sourceSelect.value === "auto") sourceSelect.value = targetSelect.value === "zh" ? "en" : "zh";
  const nextSource = targetSelect.value;
  const nextTarget = sourceSelect.value === "auto" ? "en" : sourceSelect.value;
  sourceSelect.value = nextSource;
  targetSelect.value = nextTarget === nextSource ? "en" : nextTarget;
});
document.querySelector("#speak-source").addEventListener("click", () =>
  speakText(sourceText.value, sourceSelect.value === "auto" ? "en" : sourceSelect.value).catch(showStatus),
);
document.querySelector("#history-search").addEventListener("click", () => loadHistory().catch(showStatus));
document.querySelector("#history-clear").addEventListener("click", async () => {
  await invoke("history_action", { action: { op: "clear", id: 0, query: "" } });
  loadHistory().catch(showStatus);
});
document.querySelector("#vocab-csv").addEventListener("click", () => exportVocab("csv").catch(showStatus));
document.querySelector("#vocab-anki").addEventListener("click", () => exportVocab("anki").catch(showStatus));
document.querySelector("#clear-cache").addEventListener("click", async () => {
  await invoke("clear_cache");
  status.textContent = "缓存已清空";
});
document.querySelector("#open-logs").addEventListener("click", () =>
  invoke("app_action", { action: { op: "logs", kind: "", pinned: false } }),
);
document.querySelector("#open-data").addEventListener("click", () =>
  invoke("app_action", { action: { op: "open-data", kind: "", pinned: false } }),
);
document.querySelector("#check-update").addEventListener("click", async () => {
  status.textContent = "正在检查更新";
  await invoke("app_action", { action: { op: "update", kind: "", pinned: false } });
});
document.querySelector("#page-settings").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!inDesktop()) {
    status.textContent = "预览模式不能保存设置";
    return;
  }
  try {
    await saveSettings();
  } catch (error) {
    showStatus(error);
  }
});
function captureOnboardTarget() {
  const select = document.querySelector("#onboard-target");
  if (select) targetSelect.value = select.value;
}

document.querySelector("#onboard-next").addEventListener("click", async () => {
  captureOnboardTarget();
  if (onboardStep < 2) {
    onboardStep += 1;
    renderOnboarding();
    return;
  }
  document.querySelector("#onboarding").hidden = true;
  if (inDesktop()) await saveSettings().catch(showStatus);
});
document.querySelector("#onboard-back").addEventListener("click", () => {
  captureOnboardTarget();
  onboardStep = Math.max(0, onboardStep - 1);
  renderOnboarding();
});
for (const input of document.querySelectorAll("[data-shortcut]")) {
  input.addEventListener("keydown", (event) => {
    event.preventDefault();
    const parts = [];
    if (event.ctrlKey || event.metaKey) parts.push("ctrl");
    if (event.altKey) parts.push("alt");
    if (event.shiftKey) parts.push("shift");
    const key = event.key.toLowerCase();
    if (!["control", "alt", "shift", "meta"].includes(key)) parts.push(key.length === 1 ? key : key);
    input.value = parts.join("+");
  });
}
themeToggle.addEventListener("click", () => {
  const current = localStorage.getItem(THEME_KEY) || "system";
  applyTheme(current === "dark" ? "light" : "dark");
  document.querySelector("#theme-choice").value = localStorage.getItem(THEME_KEY);
});
document.querySelector("#theme-choice").addEventListener("change", (event) => applyTheme(event.target.value));
document.querySelector("#ui-language").addEventListener("change", (event) => applyI18n(event.target.value));
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if ((localStorage.getItem(THEME_KEY) || "system") === "system") applyTheme("system");
});
sourceText.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    translate();
  }
});
let autoTimer = 0;
sourceText.addEventListener("input", () => {
  if (!settings?.auto_translate) return;
  window.clearTimeout(autoTimer);
  autoTimer = window.setTimeout(() => {
    if (sourceText.value.trim()) translate();
  }, 650);
});

const jobs = { file: "", minecraft: "" };

function jobPayload(kind) {
  if (kind === "minecraft") {
    return {
      op: "start",
      kind: "minecraft",
      input_path: document.querySelector("#mc-path").value,
      engine: document.querySelector("#mc-engine").value,
      source: "en",
      target: "zh",
      mode: "translated",
      mc_version: document.querySelector("#mc-version").value,
      glossary: document.querySelector("#mc-glossary").value,
      id: jobs.minecraft,
    };
  }
  return {
    op: "start",
    kind: "file",
    input_path: document.querySelector("#file-path").value,
    engine: document.querySelector("#file-engine").value,
    source: sourceSelect.value,
    target: document.querySelector("#file-target").value,
    mode: document.querySelector("#file-mode").value,
    mc_version: "",
    glossary: "",
    id: jobs.file,
  };
}

async function runJob(kind, op) {
  if (!inDesktop()) throw new Error("预览模式不能翻译文件");
  const action = jobPayload(kind);
  action.op = op;
  const response = await invoke("job_action", { action });
  if (response.id) jobs[kind] = response.id;
  if (response.preview) {
    renderMinecraftPreview(response);
    const box = document.querySelector("#mc-glossary");
    if (!box.value.trim() && response.glossary) box.value = response.glossary;
    return;
  }
  if (op === "start" || op === "continue") pollJob(kind).catch(showStatus);
  return response;
}

function renderMinecraftPreview(response) {
  const report = document.querySelector("#mc-report");
  report.replaceChildren();
  const terms = (response.terms || []).join("、");
  report.append(card("扫描结果", `共 ${response.entries || 0} 条，已有中文 ${response.already || 0} 条`));
  for (const source of response.sources || []) {
    report.append(card(source.kind, `${source.path}\n${source.entries} 条，已有中文 ${source.translated} 条`));
  }
  if (terms) report.append(card("高频词", terms));
}

async function pollJob(kind) {
  const id = jobs[kind];
  if (!id) return;
  const response = await invoke("job_action", { action: { ...jobPayload(kind), op: "status", id } });
  const job = response.job || {};
  const total = Number(job.total || 0);
  const finished = Number(job.done_count || 0) + Number(job.skipped_count || 0) + Number(job.failed_count || 0);
  const progress = document.querySelector(kind === "file" ? "#file-progress" : "#mc-progress");
  progress.max = Math.max(total, 1);
  progress.value = finished;
  if (job.output_path) jobs[`${kind}Output`] = job.output_path;
  const report = `${job.status || ""} ${finished}/${total} 成功 ${job.done_count || 0} 跳过 ${job.skipped_count || 0} 失败 ${job.failed_count || 0} ${job.error || ""} ${job.output_path || ""}`;
  document.querySelector(kind === "file" ? "#file-report" : "#mc-status").textContent = report;
  if (job.status === "running" || job.status === "queued") {
    window.setTimeout(() => pollJob(kind).catch(showStatus), 1000);
  }
}

document.querySelector("#file-start").addEventListener("click", () => runJob("file", "start").catch(showStatus));
document.querySelector("#file-cancel").addEventListener("click", () => runJob("file", "cancel").catch(showStatus));
document.querySelector("#file-resume").addEventListener("click", () => runJob("file", "continue").catch(showStatus));
document.querySelector("#file-open").addEventListener("click", () =>
  invoke("job_action", {
    action: { ...jobPayload("file"), op: "reveal", input_path: jobs.fileOutput || document.querySelector("#file-path").value },
  }).catch(showStatus),
);
document.querySelector("#mc-preview").addEventListener("click", () => runJob("minecraft", "preview").catch(showStatus));
document.querySelector("#mc-start").addEventListener("click", () => runJob("minecraft", "start").catch(showStatus));
document.querySelector("#mc-cancel").addEventListener("click", () => runJob("minecraft", "cancel").catch(showStatus));
document.querySelector("#mc-resume").addEventListener("click", () => runJob("minecraft", "continue").catch(showStatus));
document.querySelector("#mc-open").addEventListener("click", () =>
  invoke("job_action", {
    action: { ...jobPayload("minecraft"), op: "reveal", input_path: jobs.minecraftOutput || document.querySelector("#mc-path").value },
  }).catch(showStatus),
);
if (inDesktop()) {
  listen("tauri://drag-drop", (event) => {
    const path = event.payload?.paths?.[0];
    if (!path) return;
    const visible = document.querySelector(".content .page:not([hidden])");
    if (visible?.id === "page-files") document.querySelector("#file-path").value = path;
    if (visible?.id === "page-minecraft") document.querySelector("#mc-path").value = path;
  }).catch(() => {});
}

if (inDesktop()) {
  listen("engine-result", (event) => {
    const item = event.payload;
    pinnedCards.set(item.engine, item);
    renderResults([...pinnedCards.values()]);
  }).catch(showStatus);
}

applyTheme(localStorage.getItem(THEME_KEY) || "system");
showPage("translate");
renderResults([]);
if (!inDesktop()) {
  status.textContent = "预览模式。翻译需要在桌面端里运行";
} else {
  invoke("daemon_status")
    .then((state) => {
      status.textContent = state.running ? "本机服务已连接" : "正在启动本机服务";
    })
    .catch(showStatus);
  invoke("get_settings").then(fillSettings).catch(showStatus);
}
