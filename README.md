# Lyra Writer — protótipo

Gravador de mídia de instalação do Lyra OS, inspirado no Fedora Media Writer
(lyra-ecosystem#85). Segue o esqueleto do `vega-gtk`:

- Tauri 2 com WebKitGTK;
- barra de título própria;
- tema claro/escuro e cor de destaque lidos do GNOME;
- tokens visuais copiados de `vega/vega-gtk/ui/css/app.css`.

## Estado do protótipo

| Parte | Situação |
| --- | --- |
| Lista das edições (Desktop "Odisseia", Server "Delos") | fixa no código; a versão final lê um manifesto assinado |
| ISO local | seletor de arquivo nativo |
| Pendrives | **reais**, lidos do udisks2: só discos inteiros, removíveis, por USB, sem `HintSystem` |
| Download, assinatura, gravação, verificação | **simulados**: nenhum byte é gravado |
| Restaurar pendrive | botão sem ação |
| Traduções | só pt-BR |

## Estrutura

- `lyra-writer-core`: catálogo de imagens e filtro de dispositivos (com testes);
- `lyra-writer-gtk`: frontend Tauri (binário `lyra-writer`), interface em `ui/`.

## Rodar

```bash
cargo test -p lyra-writer-core
cargo run -p lyra-writer-gtk
```

## Para a versão final

- Gravação pelo udisks2 (`OpenDevice`, autorizada pelo polkit), sem rodar como root.
- Teste negativo de autorização.
- Manifesto e checksum assinados pela chave "Lyra OS Release" (`contracts.toml`).
- gettext em en, pt-BR e es, como no Vega.
- Empacotamento no OBS.
