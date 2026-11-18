// Ponte com o backend Rust (comandos Tauri e eventos).

const tauri = window.__TAURI__;

/** Chama um comando; erros chegam como texto. */
export async function call(cmd, args = {}) {
  try {
    return await tauri.core.invoke(cmd, args);
  } catch (error) {
    throw new Error(typeof error === "string" ? error : error?.message ?? String(error));
  }
}

/** Escuta um evento do backend; devolve a função que cancela a escuta. */
export function on(event, handler) {
  const pending = tauri.event.listen(event, (e) => handler(e.payload));
  return () => pending.then((unlisten) => unlisten());
}

/** Seletor de arquivo nativo. */
export async function pick({ filters, title } = {}) {
  return tauri.core.invoke("plugin:dialog|open", { options: { directory: false, multiple: false, filters, title } });
}

export const win = tauri.window.getCurrentWindow();
