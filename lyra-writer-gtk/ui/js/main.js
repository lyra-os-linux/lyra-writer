import { call, on, pick, win } from "./api.js";
import { svg } from "./icons.js";

const $ = (id) => document.getElementById(id);

// ---------- Montagem de nós (nunca innerHTML com dados) ----------

function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props ?? {})) {
    if (value == null || value === false) continue;
    if (key === "class") el.className = value;
    else if (key === "text") el.textContent = value;
    else if (key.startsWith("on")) el.addEventListener(key.slice(2).toLowerCase(), value);
    else if (value === true) el.setAttribute(key, "");
    else el.setAttribute(key, value);
  }
  for (const child of children.flat(Infinity)) {
    if (child == null || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

/** Ícone a partir da tabela fixa de icons.js (sem dados do usuário). */
function icon(name, size = 16) {
  const tpl = document.createElement("template");
  tpl.innerHTML = svg(name, size);
  return tpl.content.firstElementChild;
}

function toast(text) {
  const el = h("div", { class: "toast", text });
  $("toasts").append(el);
  setTimeout(() => el.remove(), 3500);
}

const gb = (bytes) => `${(bytes / 1e9).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} GB`;

// ---------- Tema e cor de destaque (seguem o GNOME, como o Vega) ----------

const ACCENTS = {
  blue: "#3584e4", teal: "#2190a4", green: "#3a944a", yellow: "#c88800", orange: "#ed5b00",
  red: "#e62d42", pink: "#d56199", purple: "#9141ac", slate: "#6f8396",
};

// Visual imersivo: sempre escuro, sobre o céu noturno do Lyra. Só a cor de
// destaque segue o GNOME.
function applyInterface({ accent } = {}) {
  document.documentElement.dataset.theme = "dark";
  const color = `color-mix(in srgb, ${ACCENTS[accent] ?? ACCENTS.blue} 80%, #fff)`;
  document.documentElement.style.setProperty("--accent", color);
}

// ---------- Janela (barra de título própria) ----------

function setupWindow() {
  const make = (name, label, fn) => h("button", { class: "wc", "aria-label": label, title: label, onclick: fn }, icon(name, 13));
  const maxBtn = make("maximize", "Maximizar", () => win.toggleMaximize());
  $("window-controls").append(make("minus", "Minimizar", () => win.minimize()), maxBtn, make("x", "Fechar", () => win.close()));
  document.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || !e.target.closest("[data-drag]") || e.target.closest("button, input, select")) return;
    if (e.detail === 2) win.toggleMaximize();
    else win.startDragging();
  });
  for (const dir of ["North", "South", "East", "West", "NorthEast", "NorthWest", "SouthEast", "SouthWest"]) {
    document.body.append(h("div", { class: "resize", "data-dir": dir, onmousedown: (e) => { e.preventDefault(); win.startResizeDragging(dir); } }));
  }
  const sync = async () => {
    const max = await win.isMaximized();
    document.body.classList.toggle("maximized", max);
    maxBtn.replaceChildren(icon(max ? "restore" : "maximize", 13));
  };
  win.onResized(sync);
  sync();

  const back = $("back-btn");
  back.append(icon("arrow-left", 18));
  back.setAttribute("aria-label", "Voltar");
  back.addEventListener("click", () => { state.mode = "write"; go("image"); });
}

// ---------- Estado e telas ----------

const state = {
  step: "image",
  images: [],
  image: "desktop",
  localIso: null,
  devices: null,
  device: null,
  progress: { phase: "download", percent: 0 },
  startedAt: 0,
  error: null,
  mode: "write",
};

const STEPS = [
  ["image", "Imagem"],
  ["device", "Pendrive"],
  ["writing", "Gravação"],
];
const PHASES = [
  ["download", "Baixando a imagem", "Baixar a imagem"],
  ["signature", "Conferindo a assinatura", "Conferir a assinatura “Lyra OS Release”"],
  ["write", "Gravando no pendrive", "Gravar no pendrive"],
  ["verify", "Verificando a gravação", "Verificar a gravação"],
];
const RESTORE_PHASES = [
  ["wipe", "Apagando a imagem", "Apagar a imagem"],
  ["partition", "Criando a partição", "Criar uma partição"],
  ["format", "Formatando", "Formatar em exFAT"],
];
const phases = () => (state.mode === "restore" ? RESTORE_PHASES : PHASES);

function go(step) {
  state.step = step;
  render();
}

const writing = () => state.step === "writing";

function selectedImage() {
  return state.images.find((i) => i.id === state.image);
}

function imageLabel() {
  if (state.image === "local") return state.localIso?.split("/").pop() ?? "ISO deste computador";
  const img = selectedImage();
  return img ? `Lyra OS ${img.version} ${img.edition}` : "";
}

function deviceObj() {
  return state.devices?.find((d) => d.object === state.device);
}

// ---------- Indicador de etapas ----------

function renderStepper() {
  $("stepper").hidden = state.mode === "restore";
  const current = state.step === "done" || state.step === "error" ? 2 : STEPS.findIndex(([id]) => id === state.step);
  const finished = state.step === "done";
  $("stepper").replaceChildren(...STEPS.flatMap(([id, label], i) => {
    const status = i < current || finished ? "ok" : i === current ? "now" : "";
    const item = h("div", { class: `stepper-item ${status}`, "aria-current": status === "now" ? "step" : null },
      h("span", { class: "stepper-dot" }, status === "ok" ? icon("check", 13) : String(i + 1)),
      h("span", { text: label }));
    return i ? [h("span", { class: `stepper-line ${i <= current ? "on" : ""}`, "aria-hidden": "true" }), item] : [item];
  }));
}

// ---------- 1. Imagem ----------

function imagePage() {
  const cards = state.images.map((img) =>
    h("button", { class: "edition", role: "radio", "aria-checked": String(state.image === img.id), onclick: () => { state.image = img.id; render(); } },
      h("span", { class: "edition-top" },
        h("span", { class: "edition-icon" }, icon(img.id === "server" ? "server" : "monitor", 26)),
        img.recommended && h("span", { class: "tag", text: "Recomendado" }),
        h("span", { class: "radio" })),
      h("span", { class: "edition-name" }, h("strong", { text: img.edition }), h("span", { text: `“${img.codename}”` })),
      h("span", { class: "edition-version", text: `Lyra OS ${img.version}` }),
      h("span", { class: "edition-desc", text: img.description })));
  const local = h("button", { class: "list-row local", role: "radio", "aria-checked": String(state.image === "local"), onclick: chooseLocal },
    h("span", { class: "ic" }, icon("folder-open", 20)),
    h("span", { class: "meta" },
      h("strong", { text: state.localIso ? state.localIso.split("/").pop() : "Usar uma ISO deste computador" }),
      h("small", { text: state.localIso ? "Clique para escolher outra" : "Para quem já baixou a imagem" })),
    state.image === "local" ? h("span", { class: "radio" }) : icon("chevron-right", 18));
  return [
    h("div", { class: "hero" },
      h("h2", { text: "Grave o Lyra OS num pendrive" }),
      h("p", { class: "lead", text: "Escolha a edição. Baixamos a imagem oficial e conferimos a assinatura antes de gravar." })),
    h("div", { class: "editions", role: "radiogroup", "aria-label": "Edição" }, cards),
    h("div", { class: "list", role: "radiogroup", "aria-label": "Outra imagem" }, local),
    restoreOffer(),
  ];
}

// Como no Fedora Media Writer: um pendrive com imagem gravada aparece com a
// opção de voltar ao uso normal.
function restoreOffer() {
  const dev = state.devices?.find((d) => d.live);
  if (!dev) return null;
  return h("div", { class: "restore-offer" },
    h("span", { class: "ic" }, icon("usb", 20)),
    h("span", { class: "meta" },
      h("strong", { text: `${dev.name} tem uma imagem gravada` }),
      h("small", { text: `${dev.live} · restaure para usar como pendrive comum` })),
    h("button", { class: "btn", onclick: () => confirmRestore(dev) }, icon("refresh", 16), "Restaurar"));
}

async function chooseLocal() {
  const path = await pick({ title: "Escolher imagem ISO", filters: [{ name: "Imagem ISO", extensions: ["iso"] }] });
  if (!path) return;
  state.localIso = path;
  state.image = "local";
  render();
}

// ---------- 2. Pendrive ----------

function devicePage() {
  let body;
  if (state.devices === null) {
    body = h("div", { class: "waiting" }, h("span", { class: "pulse" }, icon("usb", 30)), h("strong", { text: "Procurando pendrives…" }));
  } else if (!state.devices.length) {
    body = h("div", { class: "waiting" },
      h("span", { class: "pulse" }, icon("plug", 30)),
      h("strong", { text: "Conecte um pendrive" }),
      h("small", { text: "Ele aparece aqui sozinho. Use um de pelo menos 8 GB." }));
  } else {
    body = h("div", { class: "list", role: "radiogroup", "aria-label": "Pendrive" }, state.devices.map((d) =>
      h("button", { class: "list-row", role: "radio", "aria-checked": String(state.device === d.object), onclick: () => { state.device = d.object; render(); } },
        h("span", { class: "ic" }, icon("usb", 20)),
        h("span", { class: "meta" }, h("strong", { text: d.name }),
          h("small", {}, d.path, d.live && h("span", { class: "live", text: d.live }))),
        d.live && !restoring && h("span", { class: "btn sm", role: "button", tabindex: "0", title: "Voltar ao uso normal",
          onclick: (e) => { e.stopPropagation(); confirmRestore(d); },
          onkeydown: (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); confirmRestore(d); } } }, "Restaurar"),
        h("span", { class: "size", text: gb(d.size) }),
        h("span", { class: "radio" }))));
  }
  const restoring = state.mode === "restore";
  return [
    h("div", { class: "hero" },
      h("h2", { text: restoring ? "Qual pendrive restaurar?" : "Em qual pendrive?" }),
      h("p", { class: "lead", text: restoring
        ? "Restaurar apaga tudo e devolve o pendrive ao uso normal: vazio, com uma partição em exFAT."
        : "Só mostramos pendrives e cartões. Os discos deste computador nunca aparecem aqui." })),
    body,
    state.devices?.length && !restoring ? h("div", { class: "banner" }, icon("alert", 18),
      h("span", {}, h("strong", { text: "Tudo o que estiver no pendrive será apagado. " }),
        "Copie antes o que quiser guardar. Depois da instalação, “Restaurar pendrive” volta ele ao uso normal.")) : null,
  ];
}

function onDevices(list) {
  state.devices = list;
  if (!deviceObj()) state.device = list.length === 1 ? list[0].object : null;
  // Pendrive removido no meio da gravação.
  if (writing() && !list.some((d) => d.object === state.writingTo)) {
    call("write_cancel");
    state.error = "O pendrive foi desconectado durante a gravação. Ele pode ter ficado inutilizável até ser gravado de novo.";
    go("error");
    return;
  }
  if (state.step === "device" || state.step === "image") render();
}

// ---------- Confirmação ----------

function dialog({ title, text, cancel = "Cancelar", confirm, danger = false, onConfirm, extra }) {
  const prev = document.activeElement;
  const close = () => { backdrop.remove(); document.removeEventListener("keydown", onKey, true); prev?.focus?.(); };
  const onKey = (e) => { if (e.key === "Escape") { e.stopPropagation(); close(); } };
  const ok = h("button", { class: `btn ${danger ? "danger" : "primary"}`, onclick: () => { close(); onConfirm(); } }, confirm);
  const backdrop = h("div", { class: "dialog-backdrop", onclick: (e) => e.target === backdrop && close() },
    h("div", { class: "dialog", role: "alertdialog", "aria-modal": "true", "aria-labelledby": "dlg-title", "aria-describedby": "dlg-text" },
      h("h3", { id: "dlg-title", text: title }),
      extra,
      h("p", { id: "dlg-text", text }),
      h("div", { class: "row" }, h("button", { class: "btn", onclick: close }, cancel), ok)));
  document.addEventListener("keydown", onKey, true);
  document.body.append(backdrop);
  backdrop.querySelector(".btn").focus();
}

function confirmWrite() {
  const dev = deviceObj();
  dialog({
    title: "Apagar o pendrive e gravar?",
    text: `Todos os arquivos de ${dev.name} serão apagados e não poderão ser recuperados.`,
    confirm: "Apagar e gravar",
    danger: true,
    onConfirm: startWrite,
    extra: h("div", { class: "summary" },
      h("span", { class: "chip" }, icon(state.image === "server" ? "server" : state.image === "local" ? "file" : "monitor", 16), imageLabel()),
      icon("arrow", 16),
      h("span", { class: "chip" }, icon("usb", 16), `${dev.name} · ${gb(dev.size)}`)),
  });
}

function confirmRestore(dev) {
  dialog({
    title: `Restaurar ${dev.name}?`,
    text: "A imagem gravada será apagada e o pendrive volta a ser um pendrive comum, vazio, formatado em exFAT.",
    confirm: "Restaurar",
    danger: true,
    onConfirm: () => startRestore(dev),
  });
}

async function startRestore(dev) {
  state.mode = "restore";
  state.device = dev.object;
  state.writingTo = dev.object;
  state.progress = { phase: "wipe", percent: 0 };
  state.startedAt = performance.now();
  go("writing");
  await call("restore_start", { device: dev.object });
}

async function startWrite() {
  state.mode = "write";
  state.progress = { phase: "download", percent: 0 };
  state.startedAt = performance.now();
  state.writingTo = state.device;
  go("writing");
  await call("write_start", { image: state.image === "local" ? state.localIso : state.image, device: state.device });
}

// ---------- 3. Gravação ----------


function remaining(percent) {
  if (percent < 3) return "Calculando o tempo restante…";
  const elapsed = (performance.now() - state.startedAt) / 1000;
  const left = Math.max(0, Math.round((elapsed / percent) * (100 - percent)));
  return left < 60 ? `Menos de 1 minuto restante` : `Cerca de ${Math.round(left / 60)} min restantes`;
}

function ring(percent, label) {
  const el = h("div", { class: "ring", role: "progressbar", "aria-label": label, "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(percent) },
    h("div", { class: "ring-inner" }, h("strong", { text: `${percent}%` }), h("small", { text: label })));
  el.style.setProperty("--p", String(percent));
  return el;
}

function writingPage() {
  const { phase, percent } = state.progress;
  const list = phases();
  const now = Math.max(0, list.findIndex(([id]) => id === phase));
  const from = state.mode === "restore"
    ? [h("span", { class: "chip" }, icon("usb", 16), deviceObj()?.name ?? ""), icon("arrow", 16), h("span", { class: "chip" }, icon("refresh", 16), "Pendrive comum")]
    : [h("span", { class: "chip" }, icon(state.image === "server" ? "server" : "monitor", 16), imageLabel()), icon("arrow", 16), h("span", { class: "chip" }, icon("usb", 16), deviceObj()?.name ?? "")];
  return [
    h("div", { class: "writing" },
      ring(percent, list[now][1]),
      h("p", { class: "lead", text: remaining(percent) }),
      h("div", { class: "summary" }, from),
      h("ol", { class: "steps" }, list.map(([, , label], i) =>
        h("li", { class: `step ${i < now ? "ok" : i === now ? "now" : ""}` },
          h("span", { class: "dot" }, i < now ? icon("check", 12) : i === now ? h("span", { class: "spinner" }) : null), label)))),
  ];
}

function confirmCancel() {
  dialog({
    title: state.mode === "restore" ? "Cancelar a restauração?" : "Cancelar a gravação?",
    text: state.mode === "restore"
      ? "O pendrive pode ficar sem partição até ser restaurado de novo."
      : "O pendrive ficará incompleto e não vai dar boot até ser gravado de novo ou restaurado.",
    cancel: "Continuar",
    confirm: "Cancelar",
    danger: true,
    onConfirm: () => call("write_cancel"),
  });
}

// ---------- Fim ----------

function donePage() {
  if (state.mode === "restore") {
    return [h("div", { class: "result" },
      h("div", { class: "badge ok" }, icon("check", 40)),
      h("h2", { text: "Pendrive restaurado" }),
      h("p", { class: "lead", text: `${deviceObj()?.name ?? "O pendrive"} está vazio, formatado em exFAT e pronto para o uso normal.` }))];
  }
  return [h("div", { class: "result" },
    h("div", { class: "badge ok" }, icon("check", 40)),
    h("h2", { text: "Pendrive pronto!" }),
    h("p", { class: "lead", text: `${imageLabel()} foi gravado e conferido em ${deviceObj()?.name ?? "seu pendrive"}.` }),
    h("ol", { class: "howto" },
      h("li", { text: "Deixe o pendrive conectado e reinicie o computador." }),
      h("li", { text: "Abra o menu de inicialização (geralmente F12, F11, F8 ou Esc)." }),
      h("li", { text: "Escolha o pendrive e siga o instalador do Lyra OS." })))];
}

function errorPage() {
  return [h("div", { class: "result" },
    h("div", { class: "badge err" }, icon("x-circle", 40)),
    h("h2", { text: "A gravação não terminou" }),
    h("p", { class: "lead", text: state.error }))];
}

// ---------- Barra de ações ----------

function actions() {
  const btn = (cls, text, onclick, disabled, ...pre) => h("button", { class: `btn lg ${cls}`, onclick, disabled }, ...pre, text);
  const sim = h("span", { class: "sim", text: "Protótipo · gravação simulada" });
  switch (state.step) {
    case "image":
      return [
        btn("flat", "Restaurar um pendrive", () => { state.mode = "restore"; go("device"); }, false, icon("refresh", 16)),
        h("span", { class: "hint" }, sim),
        btn("primary", "Continuar", () => { state.mode = "write"; go("device"); }, state.image === "local" && !state.localIso),
      ];
    case "device":
      if (state.mode === "restore") {
        return [
          h("span", { class: "hint", text: deviceObj() ? "Sua senha será pedida para restaurar." : "" }),
          btn("danger", "Restaurar", () => confirmRestore(deviceObj()), !deviceObj(), icon("refresh", 16)),
        ];
      }
      return [
        h("span", { class: "hint", text: deviceObj() ? "Sua senha será pedida para gravar." : "" }),
        btn("danger", "Apagar e gravar", confirmWrite, !deviceObj()),
      ];
    case "writing":
      return [h("span", { class: "hint", text: "Não remova o pendrive." }), btn("", "Cancelar", confirmCancel)];
    case "done":
      if (state.mode === "restore") {
        return [
          btn("", "Concluir", () => { state.mode = "write"; go("image"); }),
          btn("primary", "Ejetar com segurança", () => toast("Pode remover o pendrive. (simulado)"), false, icon("eject", 16)),
        ];
      }
      return [
        btn("flat", "Gravar outro", () => { state.mode = "write"; go("image"); }),
        h("span", { class: "hint" }),
        btn("", "Restaurar pendrive", () => deviceObj() && confirmRestore(deviceObj())),
        btn("primary", "Ejetar com segurança", () => toast("Pode remover o pendrive. (simulado)"), false, icon("eject", 16)),
      ];
    case "error":
      return [h("span", { class: "hint" }), btn("", "Escolher outro pendrive", () => { state.mode = "write"; go("device"); }),
        btn("primary", "Tentar de novo", () => (state.mode === "restore" ? startRestore(deviceObj()) : startWrite()), !deviceObj())];
  }
  return [];
}

// ---------- Desenho ----------

let lastStep = null;
function render() {
  const pages = { image: imagePage, device: devicePage, writing: writingPage, done: donePage, error: errorPage };
  const page = $("page");
  page.replaceChildren(...pages[state.step]().filter(Boolean));
  if (lastStep !== state.step) {
    page.classList.remove("enter");
    void page.offsetWidth;
    page.classList.add("enter");
    lastStep = state.step;
    const title = page.querySelector("h2");
    if (title) { title.tabIndex = -1; title.focus({ preventScroll: true }); }
  }
  renderStepper();
  $("back-btn").hidden = state.step !== "device";
  $("crumb").textContent = "";
  $("actions").replaceChildren(...actions());
}

// ---------- Início ----------

async function main() {
  setupWindow();
  applyInterface(await call("app_interface"));
  on("interface-changed", applyInterface);
  on("devices-changed", onDevices);
  on("write-progress", (p) => { state.progress = p; if (writing()) render(); });
  on("write-done", () => go("done"));
  on("write-cancelled", () => {
    if (!writing()) return;
    toast(state.mode === "restore" ? "Restauração cancelada." : "Gravação cancelada.");
    state.mode = "write";
    go("device");
  });

  // Fechar a janela no meio da gravação pede confirmação.
  win.onCloseRequested((e) => {
    if (!writing()) return;
    e.preventDefault();
    dialog({
      title: "Sair no meio da gravação?",
      text: "A gravação será interrompida e o pendrive não vai dar boot até ser gravado de novo.",
      cancel: "Continuar gravando",
      confirm: "Sair mesmo assim",
      danger: true,
      onConfirm: async () => { await call("write_cancel"); win.destroy(); },
    });
  });

  // Enter avança quando nenhum diálogo está aberto.
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" || document.querySelector(".dialog-backdrop") || e.target.closest?.("button")) return;
    $("actions").querySelector(".btn.primary:not([disabled]), .btn.danger:not([disabled])")?.click();
  });

  state.images = await call("images");
  render();
}

main();
