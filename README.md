# Tweb

Emulador de terminal leve com modo web efêmero sob demanda.

* **Terminal:** shell real via VTE (o seu `$SHELL`, com histórico).
* **Web:** comando `web` troca a mesma janela para o DuckDuckGo. Nada é gravado em disco (sessão efêmera por aba, isoladas entre si).
* **Abas web:** chips flutuantes 162x26 sobre a página, com título; rolam de lado quando passam da borda.
* **Fullscreen limpo:** `F` num vídeo põe só o vídeo em tela cheia com controles nativos; `Esc` sai.

## Atalhos

| Teclas | Ação |
|---|---|
| `web` + Enter (no shell) | nova aba com DDG |
| `Ctrl+T` (no modo web) | nova aba com DDG |
| Clique no chip / `Ctrl+Shift+Left/Right` | trocar de aba |
| `Ctrl+W` (no modo web) | fechar aba focada (última volta ao terminal) |
| `Ctrl+Shift+T` | alternar terminal ⇄ web |
| `F` (num vídeo) / `Esc` | fullscreen limpo / sair |
| Botão direito | menu WebKit ("abrir em nova janela" = nova aba) |

No terminal, `Ctrl+W` e setas continuam do shell.

## Instalação

```sh
./install.sh
```

Suporte: Arch-family, Debian/Ubuntu recentes, Fedora ≥44. O script instala dependências (GTK4 ≥4.12, VTE, WebKitGTK ≥2.42, GStreamer, python3, toolchain Rust), compila com `cargo build --release --locked` e instala `tweb` + `web` em `/usr/local/bin`.

## Notas

* Instância única por sessão (padrão `GtkApplication`).
* Sem DRM (Widevine indisponível no WebKitGTK): Netflix/Prime/Spotify web não rodam. Web aberta (ex. YouTube) sim.
* Downloads escolhidos por você vão para `~/Downloads`.
